//! GPUI windows: options, display settings, mouse tooltip, history traffic, connection details,
//! about, update check and plugin manager.

mod about;
mod icon_select;
mod assets;
mod common;
mod display_settings;
mod history;
mod network_info;
mod options;
mod plugins;
mod prompt;
mod tooltip;
mod update;

use futures::StreamExt;
use gpui::{
    AnyWindowHandle, App, AppContext, Application, Bounds, Context, Global, Hsla, Pixels, Render,
    SharedString, Size, TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, px, rgb, size,
};
use gpui_component::{Root, Theme, ThemeMode};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    EnableMenuItem, GWL_STYLE, GetSystemMenu, GetWindowLongPtrW, HMENU, HTCAPTION, HTSYSMENU,
    ICON_BIG, ICON_SMALL, IsIconic, IsWindowVisible, IsZoomed, MENU_ITEM_FLAGS, MF_ENABLED,
    MF_GRAYED, PostMessageW, SC_CLOSE, SC_MAXIMIZE, SC_MINIMIZE, SC_MOVE, SC_RESTORE, SC_SIZE,
    SM_CXICON, SM_CXSMICON, SW_SHOWNORMAL, SendMessageW, SetForegroundWindow, SetMenuDefaultItem,
    ShowWindow, TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx,
    WM_APP, WM_INITMENUPOPUP, WM_NCDESTROY, WM_NCRBUTTONDOWN, WM_NCRBUTTONUP, WM_NULL,
    WM_SETICON, WM_SYSCOMMAND, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_THICKFRAME,
};

use crate::app::{self, UiCommand};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Slot {
    Options,
    DisplaySettings,
    History,
    NetworkInfo,
    About,
    IconSelect,
    Update,
    PluginManager,
    PluginInfo,
    /// Empty window that only hosts an alert or confirm when nothing else is open.
    Prompt,
}

#[derive(Default)]
struct OpenWindows {
    windows: Vec<(Slot, AnyWindowHandle, isize)>,
}

impl Global for OpenWindows {}

pub fn run() {
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<UiCommand>();
    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            gpui_component::set_locale(crate::i18n::component_locale());
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
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
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
        UiCommand::OpenIconSelect => icon_select::open(cx),
        UiCommand::NotifyIconChanged => {
            let handles: Vec<isize> = cx
                .global::<OpenWindows>()
                .windows
                .iter()
                .map(|w| w.2)
                .collect();
            for h in handles {
                set_window_icon(HWND(h as *mut _));
            }
            cx.refresh_windows();
        }
        UiCommand::CheckUpdate { silent } => update::check(silent, cx),
        UiCommand::ShowTooltip { x, y } => tooltip::show(x, y, cx),
        UiCommand::HideTooltip => tooltip::hide(),
        UiCommand::Refresh => {
            tooltip::refresh(cx);
            cx.refresh_windows();
        }
        UiCommand::ThemeChanged => {
            setup_theme(cx);
            let handles: Vec<isize> = cx
                .global::<OpenWindows>()
                .windows
                .iter()
                .map(|w| w.2)
                .collect();
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
        UiCommand::Alert { text, warning } => prompt::show_alert(text, warning, cx),
        UiCommand::ConfirmRestartExplorer { text } => {
            prompt::show_confirm(text, true, cx, |_, _| crate::app::restart_explorer());
        }
        UiCommand::Quit => {
            tooltip::destroy(cx);
            cx.quit();
        }
    }
}

fn prune(cx: &mut App) {
    let alive: Vec<AnyWindowHandle> = cx.windows();
    cx.global_mut::<OpenWindows>()
        .windows
        .retain(|(_, h, _)| alive.contains(h));
}

fn find(slot: Slot, cx: &mut App) -> Option<AnyWindowHandle> {
    prune(cx);
    cx.global::<OpenWindows>()
        .windows
        .iter()
        .find(|w| w.0 == slot)
        .map(|w| w.1)
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
    let first = {
        let windows = &cx.global::<OpenWindows>().windows;
        windows
            .iter()
            .find(|w| w.0 != Slot::Prompt)
            .or_else(|| windows.first())
            .map(|w| (w.1, w.2))
    };
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

/// Sets the taskbar-button / Alt+Tab icon to the notify icon that matches the taskbar theme.
pub fn set_window_icon(hwnd: HWND) {
    let light = crate::win::system_uses_light_theme();
    unsafe {
        let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96);
        let small = crate::tray::load_notify_icon(light, GetSystemMetricsForDpi(SM_CXSMICON, dpi));
        let big = crate::tray::load_notify_icon(light, GetSystemMetricsForDpi(SM_CXICON, dpi));
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(LPARAM(small.0 as isize)),
        );
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(LPARAM(big.0 as isize)),
        );
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
        window_min_size: Some(size(
            window_size.width.min(px(320.)),
            window_size.height.min(px(200.)),
        )),
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
        let hwnd = handle
            .update(cx, |_, window, _| hwnd_of(window))
            .unwrap_or(0);
        if hwnd != 0 {
            let h = HWND(hwnd as *mut _);
            set_window_icon(h);
            install_system_menu(h);
            unsafe {
                // The first window a process shows takes the launcher's STARTUPINFO show state.
                if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_SHOWNORMAL);
                }
                let _ = SetForegroundWindow(h);
            }
        }
        cx.global_mut::<OpenWindows>()
            .windows
            .push((slot, any, hwnd));
    }
}

/// Posted to the window after a caption right-click, so the button-up that
/// opened the menu is not also the click that dismisses it.
const WM_SHOW_SYS_MENU: u32 = WM_APP + 20;
const SYS_MENU_SUBCLASS_ID: usize = 1;

/// GPUI paints the caption in the client area and returns `HTCAPTION` from
/// `WM_NCHITTEST`. `DefWindowProc` does not open the window menu for that
/// caption, so show `GetSystemMenu` ourselves.
fn install_system_menu(hwnd: HWND) {
    unsafe {
        if !SetWindowSubclass(hwnd, Some(system_menu_proc), SYS_MENU_SUBCLASS_ID, 0).as_bool() {
            log::warn!("failed to install the window-menu subclass");
        }
    }
}

unsafe extern "system" fn system_menu_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    unsafe {
        if msg == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(system_menu_proc), SYS_MENU_SUBCLASS_ID);
        } else if is_caption_hit(wparam.0 as u32)
            && (msg == WM_NCRBUTTONDOWN || msg == WM_NCRBUTTONUP)
        {
            if msg == WM_NCRBUTTONUP {
                let _ = PostMessageW(Some(hwnd), WM_SHOW_SYS_MENU, WPARAM(0), lparam);
            }
            return LRESULT(0);
        } else if msg == WM_SHOW_SYS_MENU {
            show_system_menu(hwnd, lparam);
            return LRESULT(0);
        } else if msg == WM_INITMENUPOPUP {
            let menu = HMENU(wparam.0 as *mut _);
            let system = GetSystemMenu(hwnd, false);
            if !system.is_invalid() && menu == system {
                update_system_menu(hwnd, menu);
                return LRESULT(0);
            }
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

fn is_caption_hit(hit: u32) -> bool {
    hit == HTCAPTION || hit == HTSYSMENU
}

fn show_system_menu(hwnd: HWND, at: LPARAM) {
    let x = (at.0 as i16) as i32;
    let y = ((at.0 >> 16) as i16) as i32;
    crate::tray::menu::apply_menu_theme();
    unsafe {
        let menu = GetSystemMenu(hwnd, false);
        if menu.is_invalid() {
            return;
        }
        update_system_menu(hwnd, menu);
        let _ = ReleaseCapture();
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenuEx(
            menu,
            (TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY).0,
            x,
            y,
            hwnd,
            None,
        );
        // Lets the menu close when the next click lands outside it.
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        if cmd.0 != 0 {
            let _ = PostMessageW(
                Some(hwnd),
                WM_SYSCOMMAND,
                WPARAM(cmd.0 as usize),
                LPARAM(0),
            );
        }
    }
}

fn update_system_menu(hwnd: HWND, menu: HMENU) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let maximized = IsZoomed(hwnd).as_bool();
        let minimized = IsIconic(hwnd).as_bool();
        let restored = !maximized && !minimized;
        let can_size = style & WS_THICKFRAME.0 != 0;
        let can_minimize = style & WS_MINIMIZEBOX.0 != 0;
        let can_maximize = style & WS_MAXIMIZEBOX.0 != 0;
        let _ = EnableMenuItem(menu, SC_RESTORE, menu_flag(!restored));
        let _ = EnableMenuItem(menu, SC_MOVE, menu_flag(restored));
        let _ = EnableMenuItem(menu, SC_SIZE, menu_flag(restored && can_size));
        let _ = EnableMenuItem(menu, SC_MINIMIZE, menu_flag(!minimized && can_minimize));
        let _ = EnableMenuItem(menu, SC_MAXIMIZE, menu_flag(!maximized && can_maximize));
        let _ = EnableMenuItem(menu, SC_CLOSE, MF_ENABLED);
        let _ = SetMenuDefaultItem(menu, SC_CLOSE, 0);
    }
}

fn menu_flag(enabled: bool) -> MENU_ITEM_FLAGS {
    if enabled { MF_ENABLED } else { MF_GRAYED }
}
