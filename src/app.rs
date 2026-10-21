//! Hidden main window, timers and the glue between the Win32 side (taskbar, tray, monitor
//! thread) and the GPUI windows.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};

use futures::channel::mpsc::UnboundedSender;
use velopack::{UpdateInfo, UpdateManager};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::config::{self, DoubleClickAction, Runtime, Settings};
use crate::display_item::HardwareItem;
use crate::i18n::tr;
use crate::monitor::hardware::{HardwareFlags, HardwareState};
use crate::monitor::history::HistoryTraffic;
use crate::monitor::{self, MonitorCmd, MonitorEvent, MonitorParams, Selection};
use crate::plugin::{self, PluginHost};
use crate::state::{self, AppState};
use crate::taskbar::{self, render::RenderMode};
use crate::tray;
use crate::win;

pub const MAIN_CLASS: PCWSTR = w!("TrafficBar_MainWnd");
const WM_APP_QUEUE: u32 = WM_APP + 10;
const WM_APP_MONITOR: u32 = WM_APP + 11;
pub const WM_APP_ACTIVATE: u32 = WM_APP + 12;
const TIMER_TASKBAR: usize = 1;
const TIMER_MAIN: usize = 2;
const TIMER_REOPEN: usize = 3;
/// `DELETE_NOTIFY_ICON_TIMER`: remove an icon that was added only so a balloon could show.
const TIMER_HIDE_NOTIFY: usize = 4;
const HIDE_NOTIFY_ICON_DELAY_MS: u32 = 8000;
const MAX_INSERT_RETRY: u32 = 5;

/// Commands for the GPUI side.
#[derive(Clone, Debug)]
pub enum UiCommand {
    OpenOptions(usize),
    OpenNetworkInfo,
    OpenHistory,
    OpenDisplaySettings,
    OpenPluginManager,
    OpenPluginInfo(usize),
    OpenAbout,
    OpenIconSelect,
    NotifyIconChanged,
    CheckUpdate { silent: bool },
    ShowTooltip { x: i32, y: i32 },
    HideTooltip,
    Refresh,
    ThemeChanged,
    Activate,
    Quit,
}

/// Work for the main window procedure (runs outside any GPUI borrow).
pub enum MainMsg {
    RenderFailed { mode: RenderMode, code: i32 },
    DoubleClick,
    SettingsChanged { reopen_taskbar: bool },
    ShowPluginOptions { plugin: usize, parent: isize },
    MessageBox { text: String, warning: bool },
}

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static MONITOR_QUEUE: Mutex<VecDeque<MonitorEvent>> = Mutex::new(VecDeque::new());

thread_local! {
    static UI_TX: RefCell<Option<UnboundedSender<UiCommand>>> = const { RefCell::new(None) };
    static QUEUE: RefCell<VecDeque<MainMsg>> = const { RefCell::new(VecDeque::new()) };
    static TICKS: RefCell<Ticks> = RefCell::new(Ticks::default());
}

#[derive(Default)]
struct Ticks {
    taskbar: u64,
    main: u64,
    insert_retry: u32,
    secondary_count: Option<usize>,
    /// memory, cpu, gpu, hdd, main board
    last_values: [i32; 5],
    notify_times: [i64; 5],
    last_today_traffic: u64,
    reopen_pending: bool,
}

fn main_hwnd() -> HWND {
    HWND(MAIN_HWND.load(Ordering::Relaxed) as *mut _)
}

pub fn send_ui(cmd: UiCommand) {
    UI_TX.with(|t| {
        if let Some(tx) = t.borrow().as_ref() {
            let _ = tx.unbounded_send(cmd);
        }
    });
}

/// Queues work for the main window; safe to call from GPUI handlers.
pub fn post(msg: MainMsg) {
    QUEUE.with(|q| q.borrow_mut().push_back(msg));
    unsafe {
        let _ = PostMessageW(Some(main_hwnd()), WM_APP_QUEUE, WPARAM(0), LPARAM(0));
    }
}

fn post_monitor(ev: MonitorEvent) {
    MONITOR_QUEUE.lock().unwrap().push_back(ev);
    let h = MAIN_HWND.load(Ordering::Relaxed);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(h as *mut _)),
                WM_APP_MONITOR,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

pub fn monitor_params(s: &Settings) -> MonitorParams {
    let g = &s.general;
    MonitorParams {
        interval_ms: g
            .monitor_time_span
            .clamp(config::MONITOR_TIME_SPAN_MIN, config::MONITOR_TIME_SPAN_MAX)
            as u32,
        cpu_by_time: g.cpu_usage_acquire_method == 0,
        show_all_interface: g.show_all_interface,
        connections_hide: g.connections_hide.clone(),
        hard_disk_name: g.hard_disk_name.clone(),
        cpu_core_name: g.cpu_core_name.clone(),
        hardware: HardwareFlags {
            cpu: g.is_hardware_enabled(HardwareItem::Cpu),
            gpu: g.is_hardware_enabled(HardwareItem::Gpu),
            hdd: g.is_hardware_enabled(HardwareItem::Hdd),
            mbd: g.is_hardware_enabled(HardwareItem::MainBoard),
        },
    }
}

fn initial_selection(s: &Settings) -> Selection {
    if s.cfg.select_all && !s.general.show_all_interface {
        Selection::All
    } else if s.cfg.auto_select {
        Selection::Auto
    } else {
        Selection::Named(s.cfg.connection_name.clone())
    }
}

pub fn runtime() -> Runtime {
    Runtime {
        light_theme: win::system_uses_light_theme(),
        win11_taskbar: taskbar::is_windows11_taskbar(),
        version: win::windows_version(),
        theme_color: win::theme_color(),
        elevated: win::is_elevated(),
        web_experience_detected: win::is_windows_web_experience_detected(),
    }
}

/// Builds the application state, the hidden main window, the tray icon, the monitor thread and
/// the taskbar window. Must run on the GPUI main thread.
pub fn start(ui_tx: UnboundedSender<UiCommand>) {
    UI_TX.with(|t| *t.borrow_mut() = Some(ui_tx));

    let (config_dir, portable, writable) = config::resolve_config_dir();
    let rt = runtime();
    let plugins_disabled = {
        let ini = config::ini::Ini::load(&config_dir.join("config.ini"));
        ini.get_string("config", "plugin_disabled", "")
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    };
    let plugins = Arc::new(PluginHost::load(&config_dir, &plugins_disabled));
    plugin::set_notify_handler(|msg| {
        let title = tr("IDS_TRAFFICMONITOR_PLUGIN_NITIFICATION");
        let h = MAIN_HWND.load(Ordering::Relaxed);
        NOTIFY_QUEUE.lock().unwrap().push_back((title, msg));
        if h != 0 {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(h as *mut _)),
                    WM_APP_MONITOR,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    });

    let mut settings = Settings::load(config_dir.clone(), portable, &plugins.item_labels(), &rt);
    settings.general.auto_run = get_auto_run(settings.general.auto_run_by_task_scheduler);
    if settings.taskbar.auto_adapt_light_theme && settings.other.last_light_mode != rt.light_theme {
        let idx = if rt.light_theme {
            settings.taskbar.light_default_style
        } else {
            settings.taskbar.dark_default_style
        };
        settings.apply_default_style(idx);
    }
    settings.other.last_light_mode = rt.light_theme;
    win::set_efficiency_mode(settings.general.efficiency_mode);

    let history = HistoryTraffic::load(settings.history_path());
    let (today_up, today_down) = history.today_bytes();
    let params = monitor_params(&settings);
    let selection = initial_selection(&settings);

    let hwnd = create_main_window();
    MAIN_HWND.store(hwnd.0 as isize, Ordering::Relaxed);

    let snapshot = monitor::Snapshot {
        today_up_traffic: today_up,
        today_down_traffic: today_down,
        ..Default::default()
    };
    plugins.send_settings(&settings.taskbar);
    plugin::update_settings(&settings.taskbar, 96, rt.theme_color, 0);

    state::init(AppState {
        settings,
        rt: rt.clone(),
        snapshot,
        plugins: plugins.clone(),
        connections: Vec::new(),
        selected_connection: 0,
        selection: selection.clone(),
        hardware_state: HardwareState::Disabled,
        disk_names: Vec::new(),
        cpu_core_names: Vec::new(),
        main_hwnd: hwnd.0 as isize,
        taskbar_hwnd: 0,
        start_time: chrono::Local::now(),
        exe_dir_writable: writable,
        render_mode: RenderMode::Layered,
        checking_update: false,
        monitor: None,
        revision: 0,
    });

    let handle = monitor::start(params, selection, history, plugins, Box::new(post_monitor));
    state::with_mut(|s| s.monitor = Some(handle));

    tray::create(hwnd, rt.light_theme);
    taskbar::open(RenderMode::Layered);
    after_taskbar_opened();

    unsafe {
        SetTimer(Some(hwnd), TIMER_TASKBAR, 100, None);
        SetTimer(Some(hwnd), TIMER_MAIN, 1000, None);
    }

    if state::with(|s| s.settings.general.check_update_when_start) {
        send_ui(UiCommand::CheckUpdate { silent: true });
    }
}

static NOTIFY_QUEUE: Mutex<VecDeque<(String, String)>> = Mutex::new(VecDeque::new());

fn after_taskbar_opened() {
    let (hwnd, dpi) = taskbar::with(|t| (t.hwnd.0 as isize, t.dpi)).unwrap_or((0, 96));
    state::with(|s| {
        plugin::update_settings(&s.settings.taskbar, dpi as i32, s.rt.theme_color, hwnd)
    });
}

fn create_main_window() -> HWND {
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(main_wndproc),
            hInstance: hinstance.into(),
            lpszClassName: MAIN_CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            MAIN_CLASS,
            w!("TrafficBar"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
        .unwrap_or_default()
    }
}

fn taskbar_created_msg() -> u32 {
    use std::sync::OnceLock;
    static ID: OnceLock<u32> = OnceLock::new();
    *ID.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

unsafe extern "system" fn main_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_QUEUE => {
            loop {
                let next = QUEUE.with(|q| q.borrow_mut().pop_front());
                match next {
                    Some(m) => handle_main_msg(m),
                    None => break,
                }
            }
            LRESULT(0)
        }
        WM_APP_MONITOR => {
            loop {
                let next = MONITOR_QUEUE.lock().unwrap().pop_front();
                match next {
                    Some(ev) => handle_monitor_event(ev),
                    None => break,
                }
            }
            loop {
                let next = NOTIFY_QUEUE.lock().unwrap().pop_front();
                match next {
                    Some((title, message)) => show_notify(&title, &message),
                    None => break,
                }
            }
            LRESULT(0)
        }
        WM_APP_ACTIVATE => {
            send_ui(UiCommand::Activate);
            LRESULT(0)
        }
        WM_TIMER => {
            match wparam.0 {
                TIMER_TASKBAR => on_taskbar_timer(),
                TIMER_MAIN => on_main_timer(),
                TIMER_REOPEN => {
                    unsafe {
                        let _ = KillTimer(Some(hwnd), TIMER_REOPEN);
                    }
                    TICKS.with(|t| t.borrow_mut().reopen_pending = false);
                    reopen_taskbar();
                }
                TIMER_HIDE_NOTIFY => hide_temporary_notify_icon(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        tray::WM_APP_TRAY => {
            let event = (lparam.0 & 0xFFFF) as u32;
            match event {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => send_ui(UiCommand::Activate),
                WM_RBUTTONUP => {
                    send_ui(UiCommand::HideTooltip);
                    let owner = taskbar::hwnd()
                        .filter(|_| taskbar::is_open())
                        .unwrap_or(hwnd);
                    tray::menu::show_context_menu(owner, None);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DWMCOLORIZATIONCOLORCHANGED => {
            state::with_mut(|s| s.rt.theme_color = win::theme_color());
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            check_theme();
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
            if wparam.0 == PBT_APMRESUMEAUTOMATIC {
                state::with(|s| {
                    if let Some(m) = &s.monitor {
                        m.send(MonitorCmd::Refresh);
                    }
                });
            }
            LRESULT(1)
        }
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                state::with(|s| {
                    s.save_settings();
                    if let Some(m) = &s.monitor {
                        m.send(MonitorCmd::SaveHistory);
                    }
                });
            }
            LRESULT(0)
        }
        m if m == taskbar_created_msg() && m != 0 => {
            tray::readd();
            schedule_reopen(1000);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn schedule_reopen(ms: u32) {
    let pending = TICKS.with(|t| std::mem::replace(&mut t.borrow_mut().reopen_pending, true));
    if !pending {
        unsafe {
            SetTimer(Some(main_hwnd()), TIMER_REOPEN, ms, None);
        }
    }
}

pub fn reopen_taskbar() {
    let win11 = taskbar::is_windows11_taskbar();
    let mode = state::with_mut(|s| {
        s.rt.win11_taskbar = win11;
        s.render_mode
    });
    taskbar::open(mode);
    after_taskbar_opened();
}

fn on_taskbar_timer() {
    let tick = TICKS.with(|t| {
        let mut t = t.borrow_mut();
        t.taskbar += 1;
        t.taskbar
    });
    let (secondary, has_window) = state::with(|s| {
        (
            s.settings.taskbar.show_taskbar_wnd_in_secondary_display,
            s.taskbar_hwnd != 0,
        )
    });
    if tick % 5 == 0 && secondary {
        let n = taskbar::secondary_taskbars().len();
        let changed = TICKS.with(|t| {
            let mut t = t.borrow_mut();
            let changed = t.secondary_count.is_some_and(|c| c != n);
            t.secondary_count = Some(n);
            changed
        });
        if changed {
            schedule_reopen(500);
        }
    }
    if !has_window {
        return;
    }
    if !taskbar::is_open() {
        schedule_reopen(1000);
        return;
    }
    taskbar::with(|t| {
        if tick % 10 == 0 {
            t.check_dpi();
            t.recalculate();
        }
        t.adjust_window_pos(false);
        t.render();
    });
}

fn on_main_timer() {
    let tick = TICKS.with(|t| {
        let mut t = t.borrow_mut();
        t.main += 1;
        t.main
    });
    check_theme();
    check_notifications(tick);
    tray::update_tip();

    if tick % 10 == 1 {
        let cannot_insert = taskbar::with(|t| t.cannot_insert).unwrap_or(false);
        if cannot_insert {
            let retry = TICKS.with(|t| {
                let mut t = t.borrow_mut();
                t.insert_retry += 1;
                t.insert_retry
            });
            if retry <= MAX_INSERT_RETRY {
                reopen_taskbar();
            } else if retry == MAX_INSERT_RETRY + 1 {
                let code = taskbar::with(|t| t.error_code).unwrap_or(0);
                win::message_box(
                    None,
                    &format!("{}{}", tr("IDS_CONNOT_INSERT_TO_TASKBAR"), code),
                    true,
                );
            }
        } else {
            TICKS.with(|t| t.borrow_mut().insert_retry = 0);
        }
    }
}

/// Follows Windows dark/light mode (`CheckWindows10LightTheme` + auto adapt presets).
fn check_theme() {
    let light = win::system_uses_light_theme();
    let theme_color = win::theme_color();
    let changed = state::with_mut(|s| {
        s.rt.theme_color = theme_color;
        if s.rt.light_theme == light {
            return false;
        }
        s.rt.light_theme = light;
        s.settings.other.last_light_mode = light;
        if s.settings.general.notify_icon_auto_adapt {
            let next = crate::notify_icon::adapt(&s.settings.general.notify_icon, light);
            if next != s.settings.general.notify_icon {
                s.settings.general.notify_icon = next;
            }
        }
        if s.settings.taskbar.auto_adapt_light_theme {
            let idx = if light {
                s.settings.taskbar.light_default_style
            } else {
                s.settings.taskbar.dark_default_style
            };
            s.settings.apply_default_style(idx);
        }
        s.save_settings();
        true
    });
    if changed {
        tray::set_theme(light);
        taskbar::with(|t| t.apply_settings());
        send_ui(UiCommand::ThemeChanged);
    }
}

fn check_notifications(tick: u64) {
    let data = state::with(|s| {
        (
            s.settings.general.clone(),
            s.snapshot.clone(),
            s.settings.other.notify_interval as i64,
        )
    });
    let (g, snap, interval) = data;
    let title = format!("{} {}", tr("APP_NAME"), tr("IDS_NOTIFY"));
    let mut pending: Vec<String> = Vec::new();
    TICKS.with(|t| {
        let mut t = t.borrow_mut();
        let tick = tick as i64;
        let checks = [
            (
                g.memory_usage_tip,
                snap.memory_usage,
                format!("{} {}%!", tr("IDS_MEMORY_UDAGE_EXCEED"), snap.memory_usage),
            ),
            (
                g.cpu_temp_tip,
                snap.cpu_temperature as i32,
                format!(
                    "{} {}°C!",
                    tr("IDS_CPU_TEMPERATURE_EXCEED"),
                    snap.cpu_temperature as i32
                ),
            ),
            (
                g.gpu_temp_tip,
                snap.gpu_temperature as i32,
                format!(
                    "{} {}°C!",
                    tr("IDS_GPU_TEMPERATURE_EXCEED"),
                    snap.gpu_temperature as i32
                ),
            ),
            (
                g.hdd_temp_tip,
                snap.hdd_temperature as i32,
                format!(
                    "{} {}°C!",
                    tr("IDS_HDD_TEMPERATURE_EXCEED"),
                    snap.hdd_temperature as i32
                ),
            ),
            (
                g.mainboard_temp_tip,
                snap.main_board_temperature as i32,
                format!(
                    "{} {}°C!",
                    tr("IDS_MBD_TEMPERATURE_EXCEED"),
                    snap.main_board_temperature as i32
                ),
            ),
        ];
        for (slot, (tip, value, text)) in checks.into_iter().enumerate() {
            if !tip.enable {
                continue;
            }
            let last = t.last_values[slot];
            let since = tick - t.notify_times[slot];
            if last < tip.tip_value
                && value >= tip.tip_value
                && (t.notify_times[slot] == 0 || since > interval)
            {
                pending.push(text);
                t.notify_times[slot] = tick;
            }
            t.last_values[slot] = value;
        }
        if g.traffic_tip_enable {
            let limit = if g.traffic_tip_unit == 0 {
                g.traffic_tip_value.max(0) as u64 * 1024 * 1024
            } else {
                g.traffic_tip_value.max(0) as u64 * 1024 * 1024 * 1024
            };
            let today = snap.today_up_traffic + snap.today_down_traffic;
            if t.last_today_traffic < limit && today >= limit {
                pending.push(format!(
                    "{} {}!",
                    tr("IDS_TODAY_TRAFFIC_EXCEED"),
                    crate::format::data_size_to_string(today, true)
                ));
            }
            t.last_today_traffic = today;
        }
    });
    for p in pending {
        show_notify(&title, &p);
    }
}

/// `ShowNotifyTip`. A hidden icon is put back for the balloon, then removed after 8 seconds.
fn show_notify(title: &str, message: &str) {
    if tray::notify(title, message) {
        unsafe {
            SetTimer(
                Some(main_hwnd()),
                TIMER_HIDE_NOTIFY,
                HIDE_NOTIFY_ICON_DELAY_MS,
                None,
            );
        }
    }
}

fn hide_temporary_notify_icon(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_HIDE_NOTIFY);
    }
    let show = state::try_with(|s| s.settings.general.show_notify_icon).unwrap_or(true);
    if !show {
        tray::set_visible(false);
    }
}

fn handle_monitor_event(ev: MonitorEvent) {
    match ev {
        MonitorEvent::Snapshot(snap) => {
            state::with_mut(|s| {
                s.snapshot = snap;
                s.revision += 1;
            });
            send_ui(UiCommand::Refresh);
        }
        MonitorEvent::Connections {
            list,
            selected,
            selection,
        } => {
            state::with_mut(|s| {
                s.connections = list;
                s.selected_connection = selected;
                s.selection = selection;
                if s.selection != Selection::All {
                    let name = s.connection_name();
                    if !name.is_empty() {
                        s.settings.cfg.connection_name = name;
                    }
                }
                s.revision += 1;
            });
            send_ui(UiCommand::Refresh);
        }
        MonitorEvent::HardwareLists { disks, cpu_cores } => {
            state::with_mut(|s| {
                s.disk_names = disks;
                s.cpu_core_names = cpu_cores;
                s.revision += 1;
            });
        }
        MonitorEvent::HardwareState(st) => {
            state::with_mut(|s| {
                s.hardware_state = st;
                s.revision += 1;
            });
            send_ui(UiCommand::Refresh);
        }
    }
}

fn handle_main_msg(msg: MainMsg) {
    match msg {
        MainMsg::RenderFailed { mode, code } => {
            if mode == RenderMode::Layered {
                log::warn!(
                    "UpdateLayeredWindowIndirect failed ({code}), switching to DirectComposition"
                );
                state::with_mut(|s| s.render_mode = RenderMode::Composition);
                reopen_taskbar();
            } else {
                win::message_box(None, &format!("{}{:#x}", tr("BAR_D2D_FAILED"), code), true);
            }
        }
        MainMsg::DoubleClick => {
            let (action, exe) = state::with(|s| {
                (
                    s.settings.taskbar.double_click_action,
                    s.settings.taskbar.double_click_exe.clone(),
                )
            });
            match action {
                DoubleClickAction::ConnectionInfo => send_ui(UiCommand::OpenNetworkInfo),
                DoubleClickAction::HistoryTraffic => send_ui(UiCommand::OpenHistory),
                DoubleClickAction::Options => send_ui(UiCommand::OpenOptions(0)),
                DoubleClickAction::TaskManager => open_task_manager(),
                DoubleClickAction::SpecificApp => win::shell_open(&exe),
                DoubleClickAction::None => {}
            }
        }
        MainMsg::SettingsChanged {
            reopen_taskbar: reopen,
        } => {
            let (params, rt, efficiency) = state::with(|s| {
                (
                    monitor_params(&s.settings),
                    s.rt.clone(),
                    s.settings.general.efficiency_mode,
                )
            });
            win::set_efficiency_mode(efficiency);
            state::with(|s| {
                if let Some(m) = &s.monitor {
                    m.send(MonitorCmd::SetParams(params));
                }
                s.plugins.send_settings(&s.settings.taskbar);
            });
            let _ = rt;
            tray::reload();
            let show_icon = state::with(|s| s.settings.general.show_notify_icon);
            tray::set_visible(show_icon);
            send_ui(UiCommand::NotifyIconChanged);
            if reopen {
                reopen_taskbar();
            } else {
                taskbar::with(|t| t.apply_settings());
                after_taskbar_opened();
            }
        }
        MainMsg::ShowPluginOptions { plugin, parent } => {
            let plugins = state::with(|s| s.plugins.clone());
            if !plugins.show_options(plugin, parent) {
                win::message_box(None, &tr("IDS_PLUGIN_NO_OPTIONS_INFO"), false);
            }
            taskbar::with(|t| t.apply_settings());
        }
        MainMsg::MessageBox { text, warning } => win::message_box(None, &text, warning),
    }
}

pub fn show_plugin_options(plugin: usize, parent: isize) {
    post(MainMsg::ShowPluginOptions { plugin, parent });
}

pub fn open_task_manager() {
    let p = win::system_dir().join("Taskmgr.exe");
    win::shell_open(&p.to_string_lossy());
}

pub fn restart_as_admin() {
    relaunch(true);
}

/// Relaunch the current process. Used after installing a plugin while already elevated.
pub fn restart() {
    relaunch(false);
}

fn relaunch(as_admin: bool) {
    state::with(|s| {
        s.save_settings();
    });
    shutdown();
    let started = if as_admin {
        win::restart_as_admin()
    } else {
        win::restart()
    };
    if started {
        send_ui(UiCommand::Quit);
    } else {
        // The user declined the UAC prompt, or the new process failed to start.
        let hwnd = main_hwnd();
        let light = state::with(|s| s.rt.light_theme);
        tray::create(hwnd, light);
        restart_monitor();
        reopen_taskbar();
    }
}

fn restart_monitor() {
    let (params, selection, path, plugins) = state::with(|s| {
        (
            monitor_params(&s.settings),
            s.selection.clone(),
            s.settings.history_path(),
            s.plugins.clone(),
        )
    });
    let handle = monitor::start(
        params,
        selection,
        HistoryTraffic::load(path),
        plugins,
        Box::new(post_monitor),
    );
    state::with_mut(|s| s.monitor = Some(handle));
}

fn shutdown() {
    let monitor = state::with_mut(|s| s.monitor.take());
    if let Some(m) = monitor {
        m.quit();
    }
    taskbar::close();
    unsafe {
        let _ = KillTimer(Some(main_hwnd()), TIMER_HIDE_NOTIFY);
    }
    tray::destroy();
}

pub fn exit() {
    state::with(|s| {
        s.save_settings();
    });
    shutdown();
    unsafe {
        let _ = KillTimer(Some(main_hwnd()), TIMER_TASKBAR);
        let _ = KillTimer(Some(main_hwnd()), TIMER_MAIN);
    }
    send_ui(UiCommand::Quit);
}

/// Saves state, starts Velopack's updater, then kills this process without running
/// `DLL_PROCESS_DETACH` (some GPU drivers hang there longer than the updater will wait).
pub fn apply_update_and_restart(
    manager: &UpdateManager,
    update: &UpdateInfo,
) -> Result<(), String> {
    state::with(|s| {
        s.save_settings();
    });
    shutdown();
    unsafe {
        let _ = KillTimer(Some(main_hwnd()), TIMER_TASKBAR);
        let _ = KillTimer(Some(main_hwnd()), TIMER_MAIN);
    }
    manager
        .wait_exit_then_apply_updates(update, false, true, Vec::<String>::new())
        .map_err(|e| e.to_string())?;
    unsafe {
        let _ = TerminateProcess(GetCurrentProcess(), 0);
    }
    Ok(())
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "TrafficBar";

fn task_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    format!(r"TrafficBar\Autorun for {user}")
}

fn exe_path_reg() -> String {
    let p = win::exe_path().to_string_lossy().into_owned();
    if p.contains(' ') {
        format!("\"{p}\"")
    } else {
        p
    }
}

fn schtasks(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("schtasks.exe")
        .args(args)
        .creation_flags(0x0800_0000)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn get_auto_run(task_scheduler: bool) -> bool {
    if task_scheduler {
        schtasks(&["/Query", "/TN", &task_name()])
    } else {
        win::reg_string(
            windows::Win32::System::Registry::HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
        )
        .map(|v| v == exe_path_reg())
        .unwrap_or(false)
    }
}

/// Registry run key or a logon task (`SetAutoRunByRegistry` / `SetAutoRunByTaskScheduler`).
pub fn set_auto_run(enable: bool, task_scheduler: bool) -> Result<(), String> {
    use windows::Win32::System::Registry::*;
    let set_reg = |on: bool| -> Result<(), String> {
        unsafe {
            let mut key = HKEY::default();
            let p = win::wide(RUN_KEY);
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(p.as_ptr()),
                Some(0),
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                &mut key,
            )
            .is_err()
            {
                return Err(tr("IDS_AUTORUN_FAILED_NO_KEY"));
            }
            let name = win::wide(RUN_VALUE);
            let r = if on {
                let v = win::wide(&exe_path_reg());
                let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
                RegSetValueExW(key, PCWSTR(name.as_ptr()), Some(0), REG_SZ, Some(bytes))
                    .ok()
                    .map_err(|_| tr("IDS_AUTORUN_FAILED_NO_ACCESS"))
            } else {
                let _ = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
                Ok(())
            };
            let _ = RegCloseKey(key);
            r
        }
    };
    let set_task = |on: bool| -> Result<(), String> {
        let name = task_name();
        schtasks(&["/Delete", "/TN", &name, "/F"]);
        if !on {
            return Ok(());
        }
        let user = format!(
            "{}\\{}",
            std::env::var("USERDOMAIN").unwrap_or_default(),
            std::env::var("USERNAME").unwrap_or_default()
        );
        let exe = win::exe_path().to_string_lossy().into_owned();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Author>{user}</Author></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><Delay>PT3S</Delay><UserId>{user}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>false</StartWhenAvailable>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
  </Settings>
  <Actions Context="Author"><Exec><Command>{exe}</Command></Exec></Actions>
</Task>"#,
            user = xml_escape(&user),
            exe = xml_escape(&exe)
        );
        let path = std::env::temp_dir().join("TrafficBar_task.xml");
        let mut data: Vec<u8> = vec![0xFF, 0xFE];
        for u in xml.encode_utf16() {
            data.extend_from_slice(&u.to_le_bytes());
        }
        std::fs::write(&path, data).map_err(|e| e.to_string())?;
        let ok = schtasks(&[
            "/Create",
            "/TN",
            &name,
            "/XML",
            &path.to_string_lossy(),
            "/F",
        ]);
        let _ = std::fs::remove_file(&path);
        if ok {
            Ok(())
        } else {
            Err(tr("IDS_SET_AUTO_RUN_FAILED_WARNING"))
        }
    };
    if task_scheduler {
        if enable {
            set_reg(false)?;
        }
        set_task(enable)
    } else {
        if enable {
            let _ = set_task(false);
        }
        set_reg(enable)
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Applies a modified settings object coming from the options window.
pub fn apply_settings(new: Settings) {
    let reopen = state::with_mut(|s| {
        let old = std::mem::replace(&mut s.settings, new);
        let t_old = &old.taskbar;
        let t_new = &s.settings.taskbar;
        let reopen = t_old.show_taskbar_wnd_in_secondary_display
            != t_new.show_taskbar_wnd_in_secondary_display
            || t_old.secondary_display_index != t_new.secondary_display_index
            || t_old.tbar_wnd_on_left != t_new.tbar_wnd_on_left;
        if !s.save_settings() {
            let text = tr("IDS_CONNOT_SAVE_CONFIG_WARNING")
                .replace("<%file_path%>", &s.settings.config_path().to_string_lossy());
            post(MainMsg::MessageBox {
                text,
                warning: true,
            });
        }
        s.revision += 1;
        reopen
    });
    post(MainMsg::SettingsChanged {
        reopen_taskbar: reopen,
    });
}
