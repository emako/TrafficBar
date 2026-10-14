//! GPUI windows: options, display settings, mouse tooltip, history traffic, connection details,
//! about, update check and plugin manager.

mod about;
mod assets;
mod common;
mod display_settings;
mod history;
mod network_info;
mod options;
mod plugins;
mod tooltip;
mod update;

use futures::StreamExt;
use gpui::{
    AnyWindowHandle, App, AppContext, Application, Bounds, Context, Global, Hsla, Pixels, Render, SharedString, Size,
    TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, px, rgb, size,
};
use gpui_component::{Root, Theme, ThemeMode};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::WindowsAndMessaging::{
    ICON_BIG, ICON_SMALL, IsIconic, IsWindowVisible, SM_CXICON, SM_CXSMICON, SW_SHOWNORMAL, SendMessageW, SetForegroundWindow,
    ShowWindow, WM_SETICON,
};

use crate::app::{self, UiCommand};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Slot {
    Options,
    DisplaySettings,
    History,
    NetworkInfo,
    About,
    Update,
    PluginManager,
    PluginInfo,
}

#[derive(Default)]
struct OpenWindows {
    windows: Vec<(Slot, AnyWindowHandle, isize)>,
}

impl Global for OpenWindows {}

pub fn run() {
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<UiCommand>();
    Application::new().with_assets(assets::Assets).run(move |cx: &mut App| {
        gpui_component::init(cx);
        gpui_component::set_locale(if crate::i18n::is_zh() { "zh-CN" } else { "en" });
        setup_theme(cx);
        cx.set_global(OpenWindows::default());
        tooltip::ensure_window(cx);
        app::start(tx);
        cx.spawn(async move |cx| {
            while let Some(cmd) = rx.next().await {
                let quit = matches!(cmd, UiCommand::Quit);
                let _ = cx.update(|cx| handle(cmd, cx));
                if quit {
                    break;
                }
            }
        })
        .detach();
    });
}

fn setup_theme(cx: &mut App) {
    let dark = !crate::win::apps_use_light_theme();
    Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, None, cx);
    let theme = Theme::global_mut(cx);
    theme.font_size = px(14.);
    theme.font_family = crate::i18n::default_font().into();
    if dark {
        // The stock dark theme paints the window at #0A0A0A. Lift it, and the surfaces that
        // share that color, so lists and popovers stay flush with the page.
        let background: Hsla = rgb(0x141414).into();
        theme.background = background;
        theme.accordion = background;
        theme.list = background;
        theme.popover = background;
        theme.sidebar = background;
        theme.tab_active = background;
        theme.table = background;
    }
}

fn handle(cmd: UiCommand, cx: &mut App) {
    match cmd {
        UiCommand::OpenOptions(tab) => options::open(tab, cx),
        UiCommand::OpenNetworkInfo => network_info::open(cx),
        UiCommand::OpenHistory => history::open(cx),
        UiCommand::OpenDisplaySettings => display_settings::open(cx),
        UiCommand::OpenPluginManager => plugins::open_manager(cx),
        UiCommand::OpenPluginInfo(i) => plugins::open_info(i, cx),
        UiCommand::OpenAbout => about::open(cx),
        UiCommand::CheckUpdate { silent } => update::check(silent, cx),
        UiCommand::ShowTooltip { x, y } => tooltip::show(x, y, cx),
        UiCommand::HideTooltip => tooltip::hide(),
        UiCommand::Refresh => {
            tooltip::refresh(cx);
            cx.refresh_windows();
        }
        UiCommand::ThemeChanged => {
            setup_theme(cx);
            let handles: Vec<isize> = cx.global::<OpenWindows>().windows.iter().map(|w| w.2).collect();
            for h in handles {
                set_window_icon(HWND(h as *mut _));
            }
            cx.refresh_windows();
        }
        UiCommand::Activate => {
            if !activate_any(cx) {
                options::open(0, cx);
            }
        }
        UiCommand::Quit => {
            tooltip::destroy(cx);
            cx.quit();
        }
    }
}

fn prune(cx: &mut App) {
    let alive: Vec<AnyWindowHandle> = cx.windows();
    cx.global_mut::<OpenWindows>().windows.retain(|(_, h, _)| alive.contains(h));
}

fn find(slot: Slot, cx: &mut App) -> Option<AnyWindowHandle> {
    prune(cx);
    cx.global::<OpenWindows>().windows.iter().find(|w| w.0 == slot).map(|w| w.1)
}

fn bring_to_front(handle: AnyWindowHandle, hwnd: isize, cx: &mut App) {
    let h = HWND(hwnd as *mut _);
    unsafe {
        if hwnd != 0 && !IsWindowVisible(h).as_bool() {
            let _ = ShowWindow(h, SW_SHOWNORMAL);
        }
    }
    let _ = handle.update(cx, |_, window, _| window.activate_window());
    unsafe {
        let _ = SetForegroundWindow(h);
    }
}

/// Brings an already open window to the front. Returns false when none is open.
fn activate_any(cx: &mut App) -> bool {
    prune(cx);
    let first = cx.global::<OpenWindows>().windows.first().map(|w| (w.1, w.2));
    match first {
        Some((handle, hwnd)) => {
            bring_to_front(handle, hwnd, cx);
            true
        }
        None => false,
    }
}

pub fn activate(slot: Slot, cx: &mut App) -> bool {
    match find(slot, cx) {
        Some(h) => {
            let hwnd = cx
                .global::<OpenWindows>()
                .windows
                .iter()
                .find(|w| w.1 == h)
                .map(|w| w.2)
                .unwrap_or(0);
            bring_to_front(h, hwnd, cx);
            true
        }
        None => false,
    }
}

pub fn close(slot: Slot, cx: &mut App) {
    if let Some(h) = find(slot, cx) {
        let _ = h.update(cx, |_, window, _| window.remove_window());
    }
}

pub fn hwnd_of(window: &Window) -> isize {
    match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
        _ => 0,
    }
}

/// Sets the taskbar-button / Alt+Tab icon to "图标 1" matching the taskbar theme.
pub fn set_window_icon(hwnd: HWND) {
    let light = crate::win::system_uses_light_theme();
    unsafe {
        let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96);
        let small = crate::tray::load_notify_icon(light, GetSystemMetricsForDpi(SM_CXSMICON, dpi));
        let big = crate::tray::load_notify_icon(light, GetSystemMetricsForDpi(SM_CXICON, dpi));
        SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(small.0 as isize)));
        SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(big.0 as isize)));
    }
}

/// Opens (or focuses) a dialog-style window with a custom title bar.
pub fn open_window<V: Render + 'static>(
    slot: Slot,
    title: impl Into<SharedString>,
    window_size: Size<Pixels>,
    resizable: bool,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
) {
    if activate(slot, cx) {
        return;
    }
    let title: SharedString = title.into();
    let bounds = Bounds::centered(None, window_size, cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(title.clone()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        is_movable: true,
        is_resizable: resizable,
        is_minimizable: false,
        window_min_size: Some(size(window_size.width.min(px(320.)), window_size.height.min(px(200.)))),
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    };
    let result = cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| build(window, cx));
        let frame = cx.new(|_| common::Frame::new(title, view.into()));
        cx.new(|cx| Root::new(frame, window, cx))
    });
    if let Ok(handle) = result {
        let any: AnyWindowHandle = handle.into();
        let hwnd = handle.update(cx, |_, window, _| hwnd_of(window)).unwrap_or(0);
        if hwnd != 0 {
            let h = HWND(hwnd as *mut _);
            set_window_icon(h);
            unsafe {
                // The first window a process shows takes the launcher's STARTUPINFO show state.
                if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_SHOWNORMAL);
                }
                let _ = SetForegroundWindow(h);
            }
        }
        cx.global_mut::<OpenWindows>().windows.push((slot, any, hwnd));
    }
}
