//! Loads TrafficMonitor plugin dlls through the C++ bridge in `src/plugin_abi`.
//! Plugin items are text only: label/value/sample text are drawn by our Direct2D renderer and
//! the plugin's own drawing callbacks (`DrawItem` / `DrawItemEx`) are never invoked.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::sync::{Mutex, OnceLock, RwLock};

use windows::Win32::Foundation::{GetLastError, HMODULE};
use windows::Win32::System::LibraryLoader::LoadLibraryW;
use windows::core::PCWSTR;

use crate::config::TaskbarSettings;
use crate::display_item::DisplayItem;
use crate::monitor::Snapshot;
use crate::win::{from_pcwstr, wide};

#[repr(C)]
struct TmHostCallbacks {
    get_monitor_value: extern "C" fn(i32) -> f64,
    get_monitor_value_string: extern "C" fn(i32, i32) -> *const u16,
    show_notify_message: extern "C" fn(*const u16),
    get_language_id: extern "C" fn() -> u16,
    get_config_dir: extern "C" fn() -> *const u16,
    get_dpi: extern "C" fn(i32) -> i32,
    get_theme_color: extern "C" fn() -> u32,
    get_string_res: extern "C" fn(*const u16, *const u16) -> *const u16,
    get_taskbar_hwnd: extern "C" fn() -> *mut c_void,
}

#[repr(C)]
struct TmMonitorInfo {
    up_speed: u64,
    down_speed: u64,
    cpu_usage: i32,
    memory_usage: i32,
    gpu_usage: i32,
    hdd_usage: i32,
    cpu_temperature: i32,
    gpu_temperature: i32,
    hdd_temperature: i32,
    main_board_temperature: i32,
    cpu_freq: i32,
}

unsafe extern "C" {
    fn tm_set_host(callbacks: *const TmHostCallbacks);
    fn tm_get_instance(module: HMODULE) -> *mut c_void;
    fn tm_plugin_api_version(plugin: *mut c_void) -> i32;
    fn tm_plugin_get_item(plugin: *mut c_void, index: i32) -> *mut c_void;
    fn tm_plugin_data_required(plugin: *mut c_void);
    fn tm_plugin_show_options(plugin: *mut c_void, parent: *mut c_void) -> i32;
    fn tm_plugin_get_info(plugin: *mut c_void, index: i32) -> *const u16;
    fn tm_plugin_on_monitor_info(plugin: *mut c_void, info: *const TmMonitorInfo);
    fn tm_plugin_tooltip(plugin: *mut c_void) -> *const u16;
    fn tm_plugin_on_extended_info(plugin: *mut c_void, index: i32, data: *const u16);
    fn tm_plugin_command_count(plugin: *mut c_void) -> i32;
    fn tm_plugin_command_name(plugin: *mut c_void, index: i32) -> *const u16;
    fn tm_plugin_on_command(plugin: *mut c_void, index: i32, hwnd: *mut c_void);
    fn tm_plugin_command_checked(plugin: *mut c_void, index: i32) -> i32;
    fn tm_plugin_on_initialize(plugin: *mut c_void);
    fn tm_item_name(item: *mut c_void) -> *const u16;
    fn tm_item_id(item: *mut c_void) -> *const u16;
    fn tm_item_label(item: *mut c_void) -> *const u16;
    fn tm_item_value(item: *mut c_void) -> *const u16;
    fn tm_item_sample(item: *mut c_void) -> *const u16;
    fn tm_item_on_mouse_event(
        plugin: *mut c_void,
        item: *mut c_void,
        kind: i32,
        x: i32,
        y: i32,
        hwnd: *mut c_void,
        flag: i32,
    ) -> i32;
    fn tm_item_on_keyboard_event(
        plugin: *mut c_void,
        item: *mut c_void,
        key: i32,
        ctrl: i32,
        shift: i32,
        alt: i32,
        hwnd: *mut c_void,
        flag: i32,
    ) -> i32;
    fn tm_item_is_draw_graph(plugin: *mut c_void, item: *mut c_void) -> i32;
    fn tm_item_graph_value(plugin: *mut c_void, item: *mut c_void) -> f32;
    fn tm_item_double_line(plugin: *mut c_void, item: *mut c_void) -> i32;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PluginState {
    Succeed,
    ModuleLoadFailed,
    FunctionGetFailed,
    VersionNotSupport,
    Disabled,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseEvent {
    LClicked = 0,
    RClicked = 1,
    DbClicked = 2,
    WheelUp = 3,
    WheelDown = 4,
}

pub const MF_TASKBAR_WND: i32 = 1;

const EI_LABEL_TEXT_COLOR: i32 = 0;
const EI_VALUE_TEXT_COLOR: i32 = 1;
const EI_DRAW_TASKBAR_WND: i32 = 2;
const EI_CONFIG_DIR: i32 = 17;
/// `ITMPlugin::OR_OPTION_NOT_PROVIDED`
const OR_OPTION_NOT_PROVIDED: i32 = 2;

#[derive(Clone, Copy, Debug)]
struct Ptr(*mut c_void);
unsafe impl Send for Ptr {}
unsafe impl Sync for Ptr {}

#[derive(Clone, Debug)]
pub struct PluginItem {
    item: Ptr,
    plugin_index: usize,
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct PluginInfo {
    pub file_name: String,
    pub file_path: PathBuf,
    pub state: PluginState,
    pub error_code: u32,
    pub api_version: i32,
    /// name, description, author, copyright, version, url
    pub props: [String; 6],
    pub item_ids: Vec<String>,
    plugin: Ptr,
}

impl PluginInfo {
    pub fn name(&self) -> String {
        if self.props[0].is_empty() {
            self.file_name.clone()
        } else {
            self.props[0].clone()
        }
    }
    pub fn is_loaded(&self) -> bool {
        self.state == PluginState::Succeed
    }
}

pub struct PluginHost {
    pub plugins: Vec<PluginInfo>,
    pub items: Vec<PluginItem>,
}

struct HostData {
    snapshot: Snapshot,
    taskbar: Option<TaskbarSettings>,
    config_dir: Vec<u16>,
    dpi: i32,
    theme_color: u32,
    taskbar_hwnd: isize,
    language_id: u16,
}

static HOST_DATA: RwLock<Option<HostData>> = RwLock::new(None);
static NOTIFY: OnceLock<Box<dyn Fn(String) + Send + Sync>> = OnceLock::new();
static STRING_BUF: Mutex<Vec<u16>> = Mutex::new(Vec::new());

fn with_host<R>(f: impl FnOnce(&HostData) -> R, default: R) -> R {
    match HOST_DATA.read() {
        Ok(g) => match g.as_ref() {
            Some(d) => f(d),
            None => default,
        },
        Err(_) => default,
    }
}

thread_local! {
    static VALUE_BUF: std::cell::RefCell<Vec<u16>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn monitor_item(i: i32) -> Option<DisplayItem> {
    Some(match i {
        0 => DisplayItem::Up,
        1 => DisplayItem::Down,
        2 => DisplayItem::Cpu,
        3 => DisplayItem::Memory,
        4 => DisplayItem::GpuUsage,
        5 => DisplayItem::CpuTemp,
        6 => DisplayItem::GpuTemp,
        7 => DisplayItem::HddTemp,
        8 => DisplayItem::MainBoardTemp,
        9 => DisplayItem::HddUsage,
        10 => DisplayItem::CpuFreq,
        _ => return None,
    })
}

extern "C" fn cb_get_monitor_value(item: i32) -> f64 {
    with_host(
        |d| {
            let s = &d.snapshot;
            match item {
                0 => s.out_speed as f64,
                1 => s.in_speed as f64,
                2 => s.cpu_usage as f64,
                3 => s.memory_usage as f64,
                4 => s.gpu_usage as f64,
                5 => s.cpu_temperature as f64,
                6 => s.gpu_temperature as f64,
                7 => s.hdd_temperature as f64,
                8 => s.main_board_temperature as f64,
                9 => s.hdd_usage as f64,
                10 => s.cpu_freq as f64,
                11 => s.today_up_traffic as f64,
                12 => s.today_down_traffic as f64,
                _ => 0.0,
            }
        },
        0.0,
    )
}

extern "C" fn cb_get_monitor_value_string(item: i32, _is_main: i32) -> *const u16 {
    let text = with_host(
        |d| match item {
            11 => crate::format::kbytes_to_string(d.snapshot.today_up_traffic / 1024),
            12 => crate::format::kbytes_to_string(d.snapshot.today_down_traffic / 1024),
            _ => match (monitor_item(item), d.taskbar.as_ref()) {
                (Some(di), Some(t)) => crate::format::item_value_text(di, &d.snapshot, t),
                _ => String::new(),
            },
        },
        String::new(),
    );
    VALUE_BUF.with(|b| {
        let mut b = b.borrow_mut();
        *b = wide(&text);
        b.as_ptr()
    })
}

extern "C" fn cb_show_notify_message(msg: *const u16) {
    let text = unsafe { from_pcwstr(msg) };
    if let Some(f) = NOTIFY.get() {
        f(text);
    }
}

extern "C" fn cb_get_language_id() -> u16 {
    with_host(|d| d.language_id, 0)
}

extern "C" fn cb_get_config_dir() -> *const u16 {
    match HOST_DATA.read() {
        Ok(g) => g
            .as_ref()
            .map(|d| d.config_dir.as_ptr())
            .unwrap_or(std::ptr::null()),
        Err(_) => std::ptr::null(),
    }
}

extern "C" fn cb_get_dpi(_kind: i32) -> i32 {
    with_host(|d| d.dpi, 96)
}

extern "C" fn cb_get_theme_color() -> u32 {
    with_host(|d| d.theme_color, 0)
}

extern "C" fn cb_get_string_res(key: *const u16, _section: *const u16) -> *const u16 {
    let key = unsafe { from_pcwstr(key) };
    let text = crate::i18n::tr(&key);
    let mut buf = STRING_BUF.lock().unwrap();
    *buf = wide(&text);
    buf.as_ptr()
}

extern "C" fn cb_get_taskbar_hwnd() -> *mut c_void {
    with_host(|d| d.taskbar_hwnd as *mut c_void, std::ptr::null_mut())
}

pub fn set_notify_handler(f: impl Fn(String) + Send + Sync + 'static) {
    let _ = NOTIFY.set(Box::new(f));
}

pub fn update_snapshot(s: &Snapshot) {
    if let Ok(mut g) = HOST_DATA.write() {
        if let Some(d) = g.as_mut() {
            d.snapshot = s.clone();
        }
    }
}

pub fn update_settings(t: &TaskbarSettings, dpi: i32, theme_color: u32, taskbar_hwnd: isize) {
    if let Ok(mut g) = HOST_DATA.write() {
        if let Some(d) = g.as_mut() {
            d.taskbar = Some(t.clone());
            d.dpi = dpi;
            d.theme_color = theme_color;
            d.taskbar_hwnd = taskbar_hwnd;
        }
    }
}

fn string(p: *const u16) -> String {
    unsafe { from_pcwstr(p) }
}

impl PluginHost {
    pub fn empty() -> Self {
        PluginHost {
            plugins: Vec::new(),
            items: Vec::new(),
        }
    }

    pub fn load(config_dir: &std::path::Path, disabled: &[String]) -> Self {
        let plugin_cfg_dir = config_dir.join("plugins");
        let mut cfg_dir_str = plugin_cfg_dir.to_string_lossy().into_owned();
        if !cfg_dir_str.ends_with('\\') {
            cfg_dir_str.push('\\');
        }
        *HOST_DATA.write().unwrap() = Some(HostData {
            snapshot: Snapshot::default(),
            taskbar: None,
            config_dir: wide(&cfg_dir_str),
            dpi: 96,
            theme_color: crate::win::theme_color(),
            taskbar_hwnd: 0,
            language_id: crate::i18n::lang_id(),
        });
        static CALLBACKS: TmHostCallbacks = TmHostCallbacks {
            get_monitor_value: cb_get_monitor_value,
            get_monitor_value_string: cb_get_monitor_value_string,
            show_notify_message: cb_show_notify_message,
            get_language_id: cb_get_language_id,
            get_config_dir: cb_get_config_dir,
            get_dpi: cb_get_dpi,
            get_theme_color: cb_get_theme_color,
            get_string_res: cb_get_string_res,
            get_taskbar_hwnd: cb_get_taskbar_hwnd,
        };
        unsafe { tm_set_host(&CALLBACKS) };

        let mut host = PluginHost::empty();
        let dir = crate::win::plugin_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return host;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .map(|e| e.eq_ignore_ascii_case("dll"))
                    .unwrap_or(false)
            })
            .collect();
        files.sort();
        for path in files {
            let file_name = path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut info = PluginInfo {
                file_name: file_name.clone(),
                file_path: path.clone(),
                state: PluginState::Succeed,
                error_code: 0,
                api_version: 0,
                props: Default::default(),
                item_ids: Vec::new(),
                plugin: Ptr(std::ptr::null_mut()),
            };
            if disabled.iter().any(|d| d.eq_ignore_ascii_case(&file_name)) {
                info.state = PluginState::Disabled;
                host.plugins.push(info);
                continue;
            }
            let p = wide(&path.to_string_lossy());
            let module = match unsafe { LoadLibraryW(PCWSTR(p.as_ptr())) } {
                Ok(m) => m,
                Err(_) => {
                    info.state = PluginState::ModuleLoadFailed;
                    info.error_code = unsafe { GetLastError().0 };
                    host.plugins.push(info);
                    continue;
                }
            };
            let plugin = unsafe { tm_get_instance(module) };
            if plugin.is_null() {
                info.state = PluginState::FunctionGetFailed;
                info.error_code = unsafe { GetLastError().0 };
                host.plugins.push(info);
                continue;
            }
            let version = unsafe { tm_plugin_api_version(plugin) };
            info.api_version = version;
            if version <= 0 {
                info.state = PluginState::VersionNotSupport;
                host.plugins.push(info);
                continue;
            }
            info.plugin = Ptr(plugin);
            unsafe {
                tm_plugin_on_initialize(plugin);
                if version >= 2 {
                    let _ = std::fs::create_dir_all(&plugin_cfg_dir);
                    let d = wide(&cfg_dir_str);
                    tm_plugin_on_extended_info(plugin, EI_CONFIG_DIR, d.as_ptr());
                }
                for i in 0..6 {
                    info.props[i] = string(tm_plugin_get_info(plugin, i as i32));
                }
            }
            let plugin_index = host.plugins.len();
            let mut index = 0;
            loop {
                let item = unsafe { tm_plugin_get_item(plugin, index) };
                if item.is_null() || index > 256 {
                    break;
                }
                let id = string(unsafe { tm_item_id(item) });
                let name = string(unsafe { tm_item_name(item) });
                info.item_ids.push(id.clone());
                host.items.push(PluginItem {
                    item: Ptr(item),
                    plugin_index,
                    id,
                    name,
                });
                index += 1;
            }
            host.plugins.push(info);
        }
        host
    }

    pub fn item_ids(&self) -> Vec<String> {
        self.items.iter().map(|i| i.id.clone()).collect()
    }

    /// (id, default label) pairs for loading display strings.
    pub fn item_labels(&self) -> Vec<(String, String)> {
        self.items
            .iter()
            .map(|i| (i.id.clone(), string(unsafe { tm_item_label(i.item.0) })))
            .collect()
    }

    pub fn item(&self, id: &str) -> Option<&PluginItem> {
        self.items.iter().find(|i| i.id == id)
    }

    fn plugin_ptr(&self, item: &PluginItem) -> *mut c_void {
        self.plugins[item.plugin_index].plugin.0
    }

    pub fn plugin_of(&self, id: &str) -> Option<usize> {
        self.item(id).map(|i| i.plugin_index)
    }

    pub fn value_text(&self, item: &PluginItem) -> String {
        string(unsafe { tm_item_value(item.item.0) })
    }

    pub fn sample_text(&self, item: &PluginItem) -> String {
        string(unsafe { tm_item_sample(item.item.0) })
    }

    pub fn is_double_line(&self, item: &PluginItem) -> bool {
        unsafe { tm_item_double_line(self.plugin_ptr(item), item.item.0) == 1 }
    }

    pub fn graph_value(&self, item: &PluginItem) -> Option<f32> {
        unsafe {
            let p = self.plugin_ptr(item);
            if tm_item_is_draw_graph(p, item.item.0) != 0 {
                Some(tm_item_graph_value(p, item.item.0))
            } else {
                None
            }
        }
    }

    pub fn mouse_event(
        &self,
        item: &PluginItem,
        ev: MouseEvent,
        x: i32,
        y: i32,
        hwnd: isize,
    ) -> bool {
        unsafe {
            tm_item_on_mouse_event(
                self.plugin_ptr(item),
                item.item.0,
                ev as i32,
                x,
                y,
                hwnd as *mut c_void,
                MF_TASKBAR_WND,
            ) != 0
        }
    }

    pub fn key_event(
        &self,
        item: &PluginItem,
        key: i32,
        ctrl: bool,
        shift: bool,
        alt: bool,
        hwnd: isize,
    ) -> bool {
        unsafe {
            tm_item_on_keyboard_event(
                self.plugin_ptr(item),
                item.item.0,
                key,
                ctrl as i32,
                shift as i32,
                alt as i32,
                hwnd as *mut c_void,
                1,
            ) != 0
        }
    }

    pub fn tooltip_info(&self) -> String {
        let mut out = String::new();
        for p in self.plugins.iter().filter(|p| p.is_loaded()) {
            let t = string(unsafe { tm_plugin_tooltip(p.plugin.0) });
            let t = t.trim();
            if !t.is_empty() {
                out.push('\n');
                out.push_str(t);
            }
        }
        out
    }

    pub fn commands(&self, plugin_index: usize) -> Vec<(String, bool)> {
        let Some(p) = self.plugins.get(plugin_index).filter(|p| p.is_loaded()) else {
            return Vec::new();
        };
        unsafe {
            let n = tm_plugin_command_count(p.plugin.0).clamp(0, 64);
            (0..n)
                .map(|i| {
                    (
                        string(tm_plugin_command_name(p.plugin.0, i)),
                        tm_plugin_command_checked(p.plugin.0, i) != 0,
                    )
                })
                .collect()
        }
    }

    pub fn run_command(&self, plugin_index: usize, command: i32, hwnd: isize) {
        if let Some(p) = self.plugins.get(plugin_index).filter(|p| p.is_loaded()) {
            unsafe { tm_plugin_on_command(p.plugin.0, command, hwnd as *mut c_void) };
        }
    }

    /// Returns false when the plugin provides no options dialog.
    pub fn show_options(&self, plugin_index: usize, parent: isize) -> bool {
        if let Some(p) = self.plugins.get(plugin_index).filter(|p| p.is_loaded()) {
            let r = unsafe { tm_plugin_show_options(p.plugin.0, parent as *mut c_void) };
            return r != OR_OPTION_NOT_PROVIDED;
        }
        false
    }

    /// Called on the monitor thread after each acquisition, like the original.
    pub fn data_required(&self, s: &Snapshot) {
        update_snapshot(s);
        let info = TmMonitorInfo {
            up_speed: s.out_speed,
            down_speed: s.in_speed,
            cpu_usage: s.cpu_usage,
            memory_usage: s.memory_usage,
            gpu_usage: s.gpu_usage,
            hdd_usage: s.hdd_usage,
            cpu_temperature: s.cpu_temperature as i32,
            gpu_temperature: s.gpu_temperature as i32,
            hdd_temperature: s.hdd_temperature as i32,
            main_board_temperature: s.main_board_temperature as i32,
            cpu_freq: s.cpu_freq as i32,
        };
        for p in self.plugins.iter().filter(|p| p.is_loaded()) {
            unsafe {
                tm_plugin_data_required(p.plugin.0);
                tm_plugin_on_monitor_info(p.plugin.0, &info);
            }
        }
    }

    /// `CTrafficMonitorApp::SendSettingsToPlugin` (taskbar part).
    pub fn send_settings(&self, t: &TaskbarSettings) {
        let b = |v: bool| if v { "1" } else { "0" };
        let values: [(i32, String); 8] = [
            (9, b(t.speed_short_mode).into()),
            (10, b(t.separate_value_unit_with_space).into()),
            (11, b(t.value_right_align).into()),
            (12, t.digits_number.to_string()),
            (13, b(t.unit_byte).into()),
            (14, (t.speed_unit as i32).to_string()),
            (15, b(t.hide_unit).into()),
            (16, b(t.hide_percent).into()),
        ];
        for p in self.plugins.iter().filter(|p| p.is_loaded()) {
            for (k, v) in &values {
                let w = wide(v);
                unsafe { tm_plugin_on_extended_info(p.plugin.0, *k, w.as_ptr()) };
            }
        }
    }

    pub fn notify_draw_colors(&self, id: &str, label: u32, value: u32) {
        if let Some(i) = self.item(id) {
            let p = self.plugin_ptr(i);
            let l = wide(&label.to_string());
            let v = wide(&value.to_string());
            let one = wide("1");
            unsafe {
                tm_plugin_on_extended_info(p, EI_DRAW_TASKBAR_WND, one.as_ptr());
                tm_plugin_on_extended_info(p, EI_LABEL_TEXT_COLOR, l.as_ptr());
                tm_plugin_on_extended_info(p, EI_VALUE_TEXT_COLOR, v.as_ptr());
            }
        }
    }
}

unsafe impl Send for PluginHost {}
unsafe impl Sync for PluginHost {}

/// Copies plugin files out of a Velopack install tree into `plugin_dir`.
///
/// In-app updates only replace `current`, so a `plugins` folder next to `Update.exe` survives
/// those. Setup.exe repair does not: it renames the install root to `TrafficBar.<random>` and
/// deletes it after `--veloapp-install`. This runs before that hook exits, and again on every
/// launch, so an older copy is picked up from the live root or from that renamed folder.
pub fn preserve_installed_plugins(config_dir: &Path) {
    let Some(root) = crate::win::install_root() else {
        return;
    };
    let dest = crate::win::plugin_dir();
    let config_plugins = config_dir.join("plugins");
    let copied = absorb_legacy_plugins(&root, &dest, &config_plugins);
    if copied > 0 {
        log::warn!(
            "Copied {copied} plugin file(s) to {} so a reinstall does not delete them",
            dest.display()
        );
    }
}

fn absorb_legacy_plugins(root: &Path, dest: &Path, config_plugins: &Path) -> usize {
    let mut copied = 0;
    for src in plugin_dirs_under(root) {
        // Portable mode stores plugin settings in this same folder. Leave those ini files put;
        // only dlls (and other sibling files) are duplicated into the durable directory.
        let protected = same_dir(&src, config_plugins);
        copied += import_plugin_tree(&src, dest, protected, !protected);
    }
    // Setup.exe renames the previous install to `<root>.<random>` and deletes it only after the
    // install hook returns success. Copy out of that folder, but leave it intact so a failed
    // install can still roll back.
    for rollback in rollback_dirs(root) {
        for src in plugin_dirs_under(&rollback) {
            copied += import_plugin_tree(&src, dest, false, false);
        }
    }
    copied
}

/// `plugins` next to `Update.exe`, plus copies left under `current` or `app-*` when the install
/// root was not detected and dlls were stored beside the exe.
fn plugin_dirs_under(base: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![base.join("plugins"), base.join("current").join("plugins")];
    let Ok(entries) = std::fs::read_dir(base) else {
        return dirs;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("app-") && entry.path().is_dir() {
            dirs.push(entry.path().join("plugins"));
        }
    }
    dirs
}

fn rollback_dirs(root: &Path) -> Vec<PathBuf> {
    let Some(parent) = root.parent() else {
        return Vec::new();
    };
    let Some(root_name) = root.file_name() else {
        return Vec::new();
    };
    let root_name = root_name.to_string_lossy();
    let prefix = format!("{root_name}.");
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
                && path.join("Update.exe").is_file()
        })
        .collect()
}

/// Copies files from `src` into `dest`. When `protected` is set, `*.ini` stays in `src` (plugin
/// settings live there). When `delete_source` is set, a file that landed in `dest` is removed
/// from `src`, so a later install wipe cannot be the only copy and cannot restore a plugin the
/// user already removed from `dest`.
fn import_plugin_tree(src: &Path, dest: &Path, protected: bool, delete_source: bool) -> usize {
    if !src.is_dir() || same_dir(src, dest) {
        return 0;
    }
    if std::fs::create_dir_all(dest).is_err() {
        return 0;
    }
    import_dir(src, dest, protected, delete_source, 0)
}

fn import_dir(src: &Path, dest: &Path, protected: bool, delete_source: bool, depth: u32) -> usize {
    if depth > 8 {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(src) else {
        return 0;
    };
    let mut copied = 0;
    for entry in entries.flatten() {
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            if std::fs::create_dir_all(&to).is_err() {
                continue;
            }
            copied += import_dir(&from, &to, protected, delete_source, depth + 1);
            if delete_source {
                let _ = std::fs::remove_dir(&from);
            }
            continue;
        }
        if !from.is_file() || (protected && is_ini(&from)) {
            continue;
        }
        let ready = if dest_needs_file(&from, &to) {
            let ok = std::fs::copy(&from, &to).is_ok();
            if ok {
                copied += 1;
            }
            ok
        } else {
            to.is_file()
        };
        if delete_source && ready {
            let _ = std::fs::remove_file(&from);
        }
    }
    copied
}

fn is_ini(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ini"))
}

fn dest_needs_file(from: &Path, to: &Path) -> bool {
    if !to.exists() {
        return true;
    }
    let Ok(src_time) = modified(from) else {
        return false;
    };
    match modified(to) {
        Ok(dst_time) => src_time > dst_time,
        Err(_) => true,
    }
}

fn modified(path: &Path) -> std::io::Result<SystemTime> {
    path.metadata().and_then(|meta| meta.modified())
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path, body: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn legacy_dirs_include_version_folders_and_setup_rollback() {
        let root_parent = std::env::temp_dir().join(format!(
            "trafficbar-plugins-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root_parent);
        let root = root_parent.join("TrafficBar");
        touch(&root.join("Update.exe"), b"");
        touch(&root.join("plugins").join("live.dll"), b"live");
        touch(&root.join("current").join("plugins").join("cur.dll"), b"cur");
        touch(
            &root.join("app-1.0.0").join("plugins").join("old.dll"),
            b"old",
        );
        let rollback = root_parent.join("TrafficBar.abcDEF1234567890");
        touch(&rollback.join("Update.exe"), b"");
        touch(&rollback.join("plugins").join("saved.dll"), b"saved");
        // Not a Velopack rollback: no Update.exe.
        touch(
            &root_parent.join("TrafficBar.notes").join("plugins").join("nope.dll"),
            b"nope",
        );

        let dest = root_parent.join("kept");
        let copied = absorb_legacy_plugins(&root, &dest, &root_parent.join("unused-config"));
        assert_eq!(copied, 4);
        assert_eq!(std::fs::read(dest.join("live.dll")).unwrap(), b"live");
        assert_eq!(std::fs::read(dest.join("cur.dll")).unwrap(), b"cur");
        assert_eq!(std::fs::read(dest.join("old.dll")).unwrap(), b"old");
        assert_eq!(std::fs::read(dest.join("saved.dll")).unwrap(), b"saved");
        assert!(!root.join("plugins").join("live.dll").exists());
        assert_eq!(
            std::fs::read(rollback.join("plugins").join("saved.dll")).unwrap(),
            b"saved"
        );
        assert!(
            root_parent
                .join("TrafficBar.notes")
                .join("plugins")
                .join("nope.dll")
                .exists()
        );

        let _ = std::fs::remove_dir_all(&root_parent);
    }

    #[test]
    fn protected_config_dir_keeps_ini_and_does_not_delete() {
        let root_parent = std::env::temp_dir().join(format!(
            "trafficbar-plugins-protected-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root_parent);
        let src = root_parent.join("plugins");
        touch(&src.join("plugin.dll"), b"dll");
        touch(&src.join("plugin.ini"), b"ini");
        let dest = root_parent.join("dest");
        let copied = import_plugin_tree(&src, &dest, true, false);
        assert_eq!(copied, 1);
        assert_eq!(std::fs::read(dest.join("plugin.dll")).unwrap(), b"dll");
        assert!(!dest.join("plugin.ini").exists());
        assert!(src.join("plugin.dll").exists());
        assert!(src.join("plugin.ini").exists());
        let _ = std::fs::remove_dir_all(&root_parent);
    }

    #[test]
    fn newer_destination_wins_and_stale_source_is_removed() {
        let root_parent = std::env::temp_dir().join(format!(
            "trafficbar-plugins-newer-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root_parent);
        let src = root_parent.join("plugins").join("plugin.dll");
        let dest = root_parent.join("dest").join("plugin.dll");
        touch(&src, b"old");
        touch(&dest, b"new");
        let file = std::fs::File::options().write(true).open(&src).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH).unwrap();
        let copied = import_plugin_tree(src.parent().unwrap(), dest.parent().unwrap(), false, true);
        assert_eq!(copied, 0);
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        assert!(!src.exists());
        let _ = std::fs::remove_dir_all(&root_parent);
    }
}
