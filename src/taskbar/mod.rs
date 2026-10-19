//! The window embedded into the system taskbar (`CTaskBarDlg`, `CClassicalTaskbarDlg`,
//! `CWin11TaskbarDlg`).

pub mod layout;
pub mod render;

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, EnumDisplayMonitors, HDC, HMONITOR, PAINTSTRUCT, ScreenToClient,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{WM_MOUSEHOVER, WM_MOUSELEAVE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, TME_HOVER, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_CONTROL, VK_MENU,
    VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, PCWSTR, w};

use crate::config::Runtime;
use crate::display_item::ItemKey;
use crate::plugin::MouseEvent;
use crate::state;
use crate::win::{dpi_from_rect, from_wide};
use layout::{Context, GraphHistory, Layout};
use render::{Devices, Font, Rect, RenderMode, Renderer};

const CLASS_NAME: PCWSTR = w!("TrafficBarTaskbarWnd");
const HOVER_TIME_MS: u32 = 400;
const WM_TABLET_QUERYSYSTEMGESTURESTATUS: u32 = 0x02CC;

pub struct TaskbarWnd {
    pub hwnd: HWND,
    renderer: Renderer,
    font: Option<(Font, crate::config::FontInfo, u32)>,
    pub layout: Layout,
    graphs: GraphHistory,
    h_taskbar: HWND,
    rc_taskbar: RECT,
    win11: bool,
    h_bar: HWND,
    h_min: HWND,
    rc_min_ori: RECT,
    left_space: i32,
    top_space: i32,
    last_width: i32,
    last_height: i32,
    h_notify: HWND,
    h_start: HWND,
    last_notify_width: i32,
    last_start_pos: i32,
    rect: Rect,
    on_top_or_bottom: bool,
    last_on_top_or_bottom: bool,
    pub dpi: u32,
    pub cannot_insert: bool,
    pub error_code: u32,
    is_secondary: bool,
    width_changed: bool,
    pub menu_popped: bool,
    tracking: bool,
    pub destroyed: bool,
    pub render_error: Option<i32>,
}

thread_local! {
    static DEVICES: RefCell<Option<Devices>> = const { RefCell::new(None) };
    static TASKBAR: RefCell<Option<TaskbarWnd>> = const { RefCell::new(None) };
}

fn with_devices<R>(f: impl FnOnce(&Devices) -> R) -> Option<R> {
    DEVICES.with(|d| {
        let mut d = d.borrow_mut();
        if d.is_none() {
            match Devices::new() {
                Ok(dev) => *d = Some(dev),
                Err(e) => {
                    log::error!("create D2D devices failed: {e:?}");
                    return None;
                }
            }
        }
        d.as_ref().map(f)
    })
}

/// Width in pixels of each line drawn with `font_name` at `size_px`.
pub fn measure_lines(font_name: &str, size_px: f32, lines: &[String]) -> Vec<i32> {
    with_devices(|dev| match Font::from_px(dev, font_name, size_px) {
        Ok(font) => lines.iter().map(|l| font.text_width(dev, l)).collect(),
        Err(_) => Vec::new(),
    })
    .unwrap_or_default()
}

fn reset_devices() {
    DEVICES.with(|d| *d.borrow_mut() = None);
}

pub fn with<R>(f: impl FnOnce(&mut TaskbarWnd) -> R) -> Option<R> {
    TASKBAR.with(|t| match t.try_borrow_mut() {
        Ok(mut t) => t.as_mut().map(f),
        Err(_) => None,
    })
}

pub fn hwnd() -> Option<HWND> {
    TASKBAR.with(|t| t.try_borrow().ok().and_then(|t| t.as_ref().map(|t| t.hwnd)))
}

pub fn is_open() -> bool {
    TASKBAR.with(|t| {
        t.try_borrow()
            .map(|t| t.as_ref().map(|t| !t.destroyed).unwrap_or(false))
            .unwrap_or(true)
    })
}

fn register_class() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    from_wide(&buf[..n.max(0) as usize])
}

fn find_child(parent: HWND, class: PCWSTR) -> HWND {
    unsafe { FindWindowExW(Some(parent), None, class, PCWSTR::null()).unwrap_or_default() }
}

pub fn is_windows11_taskbar() -> bool {
    unsafe {
        let tray = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).unwrap_or_default();
        !tray.is_invalid()
            && !find_child(
                tray,
                w!("Windows.UI.Composition.DesktopWindowContentBridge"),
            )
            .is_invalid()
    }
}

/// `CTaskbarHelper::GetAllSecondaryDisplayTaskbar`.
pub fn secondary_taskbars() -> Vec<HWND> {
    unsafe extern "system" fn enum_wnd(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let v = unsafe { &mut *(lparam.0 as *mut Vec<(HWND, RECT)>) };
        if class_name(hwnd) == "Shell_SecondaryTrayWnd" {
            let mut r = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &mut r);
            }
            v.push((hwnd, r));
        }
        true.into()
    }
    unsafe extern "system" fn enum_mon(
        _m: HMONITOR,
        _hdc: HDC,
        rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let v = unsafe { &mut *(lparam.0 as *mut Vec<RECT>) };
        v.push(unsafe { *rect });
        true.into()
    }
    let mut bars: Vec<(HWND, RECT)> = Vec::new();
    let mut mons: Vec<RECT> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_wnd), LPARAM(&mut bars as *mut _ as isize));
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_mon),
            LPARAM(&mut mons as *mut _ as isize),
        );
    }
    let monitor_index = |r: &RECT| {
        mons.iter()
            .position(|m| {
                r.left >= m.left && r.top >= m.top && r.left < m.right && r.top < m.bottom
            })
            .unwrap_or(usize::MAX)
    };
    bars.sort_by(|a, b| {
        monitor_index(&a.1)
            .cmp(&monitor_index(&b.1))
            .then(a.1.left.cmp(&b.1.left))
            .then(a.1.top.cmp(&b.1.top))
    });
    bars.into_iter().map(|b| b.0).collect()
}

pub fn display_count() -> usize {
    unsafe extern "system" fn enum_mon(
        _m: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        unsafe { *(lparam.0 as *mut usize) += 1 };
        true.into()
    }
    let mut n = 0usize;
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_mon),
            LPARAM(&mut n as *mut _ as isize),
        );
    }
    n
}

fn find_taskbar(s: &state::AppState) -> (HWND, bool) {
    let t = &s.settings.taskbar;
    if t.show_taskbar_wnd_in_secondary_display && crate::win::is_taskbar_showing_in_all_displays() {
        let bars = secondary_taskbars();
        if !bars.is_empty() {
            let idx = (t.secondary_display_index.max(0) as usize).min(bars.len() - 1);
            return (bars[idx], true);
        }
    }
    unsafe {
        (
            FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).unwrap_or_default(),
            false,
        )
    }
}

fn window_rect(h: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(h, &mut r);
    }
    r
}

fn rw(r: &RECT) -> i32 {
    r.right - r.left
}
fn rh(r: &RECT) -> i32 {
    r.bottom - r.top
}

/// Creates the taskbar window and embeds it (`OpenTaskBarWnd` + `OnInitDialog`).
pub fn open(mode: RenderMode) {
    close();
    register_class();
    let Some((h_taskbar, is_secondary)) = state::try_with(find_taskbar) else {
        return;
    };
    let win11 = !is_secondary && is_windows11_taskbar()
        || is_secondary && crate::win::windows_version().is_windows11_or_later();
    let ex_style = match mode {
        RenderMode::Layered => WS_EX_TOOLWINDOW | WS_EX_LAYERED,
        RenderMode::Composition => WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP,
    };
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            CLASS_NAME,
            w!("TrafficBar Taskbar Window"),
            WS_POPUP | WS_CLIPSIBLINGS,
            0,
            0,
            1,
            1,
            None,
            None,
            GetModuleHandleW(None).ok().map(|h| h.into()),
            None,
        )
    };
    let hwnd = match hwnd {
        Ok(h) => h,
        Err(e) => {
            log::error!("create taskbar window failed: {e:?}");
            return;
        }
    };
    let mut t = TaskbarWnd {
        hwnd,
        renderer: Renderer::new(hwnd, mode),
        font: None,
        layout: Layout::default(),
        graphs: GraphHistory::default(),
        h_taskbar,
        rc_taskbar: window_rect(h_taskbar),
        win11,
        h_bar: HWND::default(),
        h_min: HWND::default(),
        rc_min_ori: RECT::default(),
        left_space: 0,
        top_space: 0,
        last_width: 0,
        last_height: 0,
        h_notify: HWND::default(),
        h_start: HWND::default(),
        last_notify_width: 0,
        last_start_pos: 0,
        rect: Rect::default(),
        on_top_or_bottom: true,
        last_on_top_or_bottom: true,
        dpi: 96,
        cannot_insert: false,
        error_code: 0,
        is_secondary,
        width_changed: false,
        menu_popped: false,
        tracking: false,
        destroyed: false,
        render_error: None,
    };
    t.init_taskbar_wnd();
    let parent = t.parent();
    t.cannot_insert = unsafe { SetParent(hwnd, Some(parent)).is_err() };
    t.error_code = unsafe { windows::Win32::Foundation::GetLastError().0 };
    if t.cannot_insert {
        log::warn!("SetParent to the taskbar failed, error {}", t.error_code);
    }
    t.dpi = dpi_from_rect(&t.rc_taskbar).unwrap_or(96);
    t.check_on_top_or_bottom();
    t.last_on_top_or_bottom = t.on_top_or_bottom;
    TASKBAR.with(|cell| *cell.borrow_mut() = Some(t));
    with(|t| {
        t.recalculate();
        t.rect = Rect::new(0, 0, t.layout.width, t.layout.height);
        t.adjust_window_pos(true);
    });
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    with(|t| t.render());
    state::with_mut(|s| s.taskbar_hwnd = hwnd.0 as isize);
}

pub fn close() {
    let old = TASKBAR.with(|t| t.borrow_mut().take());
    if let Some(mut t) = old {
        t.reset_taskbar_pos();
        unsafe {
            if IsWindow(Some(t.hwnd)).as_bool() {
                let _ = DestroyWindow(t.hwnd);
            }
        }
        with_devices(|dev| t.renderer.reset(dev));
    }
    state::try_with_mut(|s| s.taskbar_hwnd = 0);
}

impl TaskbarWnd {
    fn parent(&self) -> HWND {
        if self.win11 {
            self.h_taskbar
        } else {
            self.h_bar
        }
    }

    fn dpi_px(&self, px: i32) -> i32 {
        px * self.dpi as i32 / 96
    }

    fn init_taskbar_wnd(&mut self) {
        if self.win11 {
            self.h_notify = find_child(self.h_taskbar, w!("TrayNotifyWnd"));
            self.h_start = find_child(self.h_taskbar, w!("Start"));
        } else {
            self.h_bar = find_child(self.h_taskbar, w!("ReBarWindow32"));
            if self.h_bar.is_invalid() {
                self.h_bar = find_child(self.h_taskbar, w!("WorkerW"));
            }
            if self.h_bar.is_invalid() {
                self.h_bar = self.h_taskbar;
            }
            self.h_min = find_child(self.h_bar, w!("MSTaskSwWClass"));
            if self.h_min.is_invalid() {
                self.h_min = find_child(self.h_bar, w!("MSTaskListWClass"));
            }
            let rc_min = window_rect(self.h_min);
            let rc_bar = window_rect(self.h_bar);
            self.left_space = rc_min.left - rc_bar.left;
            self.top_space = rc_min.top - rc_bar.top;
        }
    }

    fn check_on_top_or_bottom(&mut self) {
        if self.win11 {
            self.on_top_or_bottom = true;
        } else if !self.h_taskbar.is_invalid() {
            let r = window_rect(self.h_taskbar);
            self.on_top_or_bottom = rw(&r) >= rh(&r);
        } else {
            self.on_top_or_bottom = true;
        }
    }

    fn reset_taskbar_pos(&mut self) {
        if self.win11 || self.h_min.is_invalid() {
            return;
        }
        let r = self.rc_min_ori;
        if rw(&r) > 0 || rh(&r) > 0 {
            unsafe {
                if self.on_top_or_bottom {
                    let _ = MoveWindow(self.h_min, self.left_space, 0, rw(&r), rh(&r), true);
                } else {
                    let _ = MoveWindow(self.h_min, 0, self.top_space, rw(&r), rh(&r), true);
                }
            }
        }
    }

    fn move_window(&self) {
        unsafe {
            let _ = MoveWindow(
                self.hwnd,
                self.rect.left,
                self.rect.top,
                self.rect.width(),
                self.rect.height(),
                true,
            );
        }
    }

    fn ensure_font(&mut self, dev: &Devices) {
        let info = state::with(|s| s.settings.taskbar.font.clone());
        let stale = match &self.font {
            Some((_, f, d)) => *f != info || *d != self.dpi,
            None => true,
        };
        if stale {
            if let Ok(font) = Font::new(dev, &info, self.dpi) {
                self.font = Some((font, info, self.dpi));
            }
        }
    }

    /// `CalculateWindowSize`.
    pub fn recalculate(&mut self) {
        let dpi = self.dpi;
        let on_top_or_bottom = self.on_top_or_bottom;
        let taskbar_width = rw(&self.rc_taskbar);
        let Some(layout) = with_devices(|dev| {
            self.ensure_font(dev);
            let (font, _, _) = self.font.as_ref()?;
            state::try_with(|s| {
                let ctx = Context {
                    settings: &s.settings.taskbar,
                    snapshot: &s.snapshot,
                    plugins: &s.plugins,
                    rt: &s.rt,
                    dpi,
                    on_top_or_bottom,
                    taskbar_width,
                };
                layout::calculate(dev, font, &ctx)
            })
        })
        .flatten() else {
            return;
        };
        if layout.width != self.layout.width || layout.height != self.layout.height {
            self.width_changed = true;
        }
        self.rect.right = self.rect.left + layout.width;
        self.rect.bottom = self.rect.top + layout.height;
        self.layout = layout;
    }

    pub fn apply_settings(&mut self) {
        self.font = None;
        self.graphs.clear();
        self.recalculate();
        self.adjust_window_pos(true);
        self.render();
    }

    /// `AdjustWindowPos`.
    pub fn adjust_window_pos(&mut self, force: bool) {
        if !unsafe { IsWindow(Some(self.hwnd)).as_bool() } {
            return;
        }
        let mut force = force || self.width_changed;
        if force {
            self.reset_taskbar_pos();
        }
        self.rc_taskbar = window_rect(self.h_taskbar);
        self.check_on_top_or_bottom();
        if force || self.on_top_or_bottom != self.last_on_top_or_bottom {
            self.recalculate();
            self.last_on_top_or_bottom = self.on_top_or_bottom;
            force = true;
        }
        let tb = state::with(|s| s.settings.taskbar.clone());
        let rt = state::with(|s| s.rt.clone());
        if self.win11 {
            self.adjust_win11(force, &tb, &rt);
        } else {
            self.adjust_classic(force, &tb, &rt);
        }
        if self.cannot_insert {
            let rp = window_rect(self.parent());
            let mut r = self.rect;
            r.left += rp.left;
            r.right += rp.left;
            r.top += rp.top;
            r.bottom += rp.top;
            unsafe {
                let _ = MoveWindow(self.hwnd, r.left, r.top, r.width(), r.height(), true);
                if GetForegroundWindow() == self.h_taskbar {
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE,
                    );
                }
            }
        }
        self.width_changed = false;
    }

    fn adjust_classic(&mut self, force: bool, tb: &crate::config::TaskbarSettings, rt: &Runtime) {
        let rc_min = window_rect(self.h_min);
        let rc_bar = window_rect(self.h_bar);
        let w = self.layout.width;
        let h = self.layout.height;
        self.rect.right = self.rect.left + w;
        self.rect.bottom = self.rect.top + h;
        unsafe {
            if self.on_top_or_bottom {
                if force || rw(&rc_min) != self.last_width {
                    self.rc_min_ori = rc_min;
                    self.left_space = rc_min.left - rc_bar.left;
                    self.last_width = rw(&rc_min) - w;
                    if !tb.tbar_wnd_on_left {
                        let _ = MoveWindow(
                            self.h_min,
                            self.left_space,
                            0,
                            rw(&rc_min) - w,
                            rh(&rc_min),
                            true,
                        );
                        self.move_to_x(self.left_space + rw(&rc_min) - w + 2);
                    } else {
                        let _ = MoveWindow(
                            self.h_min,
                            self.left_space + w,
                            0,
                            rw(&rc_min) - w,
                            rh(&rc_min),
                            true,
                        );
                        self.move_to_x(self.left_space);
                    }
                    self.move_to_y((rh(&rc_bar) - h) / 2);
                    if tb.horizontal_arrange && rt.version.is_windows7() {
                        self.move_to_y(self.rect.top + self.dpi_px(1));
                    }
                    self.move_window();
                }
            } else if force || rh(&rc_min) != self.last_height {
                self.rc_min_ori = rc_min;
                self.top_space = rc_min.top - rc_bar.top;
                self.last_height = rh(&rc_min) - h;
                if !tb.tbar_wnd_on_left {
                    let _ = MoveWindow(
                        self.h_min,
                        0,
                        self.top_space,
                        rw(&rc_min),
                        rh(&rc_min) - h,
                        true,
                    );
                    self.move_to_y(self.top_space + rh(&rc_min) - h + 2);
                } else {
                    let _ = MoveWindow(
                        self.h_min,
                        0,
                        self.top_space + h,
                        rw(&rc_min),
                        rh(&rc_min) - h,
                        true,
                    );
                    self.move_to_y(self.top_space);
                }
                self.move_to_x((rw(&rc_min) - w) / 2);
                if self.rect.left < self.dpi_px(2) {
                    self.move_to_x(self.dpi_px(2));
                }
                self.move_window();
            }
        }
    }

    fn adjust_win11(&mut self, force: bool, tb: &crate::config::TaskbarSettings, rt: &Runtime) {
        let rc_notify = window_rect(self.h_notify);
        let mut rc_start = window_rect(self.h_start);
        let start_w = rw(&rc_start);
        let start_h = rh(&rc_start);
        rc_start.left -= self.rc_taskbar.left;
        rc_start.top -= self.rc_taskbar.top;
        rc_start.right = rc_start.left + start_w;
        rc_start.bottom = rc_start.top + start_h;
        self.rect.right = self.rect.left + self.layout.width;
        self.rect.bottom = self.rect.top + self.layout.height;
        if !(force
            || rw(&rc_notify) != self.last_notify_width
            || rc_start.left != self.last_start_pos)
        {
            return;
        }
        self.last_notify_width = rw(&rc_notify);
        self.last_start_pos = rc_start.left;
        let center = crate::win::is_taskbar_center_align();
        let widgets = crate::win::is_taskbar_widgets_btn_shown(rt.web_experience_detected);
        let w = self.rect.width();
        if !tb.tbar_wnd_on_left || !center {
            let mut notify_x = rc_notify.left - self.rc_taskbar.left;
            if self.h_notify.is_invalid() || rc_notify.left == 0 {
                notify_x = if self.is_secondary {
                    rw(&self.rc_taskbar) - self.dpi_px(88)
                } else {
                    rw(&self.rc_taskbar) - self.dpi_px(tb.taskbar_right_space_win11)
                };
            }
            if tb.avoid_overlap_with_widgets && widgets && !center {
                self.move_to_x(notify_x - w + 2 - self.dpi_px(tb.taskbar_left_space_win11));
            } else {
                self.move_to_x(notify_x - w + 2);
            }
        } else if tb.tbar_wnd_snap {
            self.move_to_x(rc_start.left - w - 2);
        } else if widgets {
            self.move_to_x(2 + self.dpi_px(tb.taskbar_left_space_win11));
        } else {
            self.move_to_x(2);
        }
        self.move_to_x(self.rect.left + self.dpi_px(tb.window_offset_left));
        let y = (start_h - self.rect.height()) / 2
            + (rh(&self.rc_taskbar) - start_h)
            + self.dpi_px(tb.window_offset_top);
        let y = if self.h_start.is_invalid() {
            (rh(&self.rc_taskbar) - self.rect.height()) / 2 + self.dpi_px(tb.window_offset_top)
        } else {
            y
        };
        self.move_to_y(y);
        self.move_window();
    }

    fn move_to_x(&mut self, x: i32) {
        let w = self.rect.width();
        self.rect.left = x;
        self.rect.right = x + w;
    }

    fn move_to_y(&mut self, y: i32) {
        let h = self.rect.height();
        self.rect.top = y;
        self.rect.bottom = y + h;
    }

    /// Re-reads the DPI of the monitor the taskbar is on (`CheckWindowMonitorDPIAndHandle`).
    pub fn check_dpi(&mut self) {
        if let Some(dpi) = dpi_from_rect(&self.rc_taskbar) {
            if dpi != self.dpi {
                self.dpi = dpi;
                self.font = None;
                self.recalculate();
                self.adjust_window_pos(true);
            }
        }
    }

    pub fn render(&mut self) {
        if self.destroyed || self.render_error.is_some() {
            return;
        }
        let size = (self.layout.width, self.layout.height);
        let dpi = self.dpi;
        let on_top_or_bottom = self.on_top_or_bottom;
        let taskbar_width = rw(&self.rc_taskbar);
        let layout = std::mem::take(&mut self.layout);
        let mut graphs = std::mem::take(&mut self.graphs);
        let result = with_devices(|dev| {
            self.ensure_font(dev);
            let Some((font, _, _)) = self.font.as_ref() else {
                return Ok(());
            };
            let Some(r) = state::try_with(|s| {
                let ctx = Context {
                    settings: &s.settings.taskbar,
                    snapshot: &s.snapshot,
                    plugins: &s.plugins,
                    rt: &s.rt,
                    dpi,
                    on_top_or_bottom,
                    taskbar_width,
                };
                let background = if s.settings.taskbar.is_transparent(&s.rt) {
                    None
                } else {
                    Some(s.settings.taskbar.back_color)
                };
                self.renderer.render(
                    dev,
                    font,
                    size,
                    background,
                    s.settings.taskbar.enable_colorful_emoji,
                    |frame| {
                        layout::draw(frame, &layout, &ctx, &mut graphs);
                    },
                )
            }) else {
                return Ok(());
            };
            r
        });
        self.layout = layout;
        self.graphs = graphs;
        match result {
            Some(Ok(())) => {}
            Some(Err(code)) => self.on_render_error(code),
            None => {}
        }
    }

    fn on_render_error(&mut self, code: i32) {
        const D2DERR_RECREATE_TARGET: i32 = 0x8899000Cu32 as i32;
        const DXGI_ERROR_DEVICE_REMOVED: i32 = 0x887A0005u32 as i32;
        const DXGI_ERROR_DEVICE_RESET: i32 = 0x887A0007u32 as i32;
        const ERROR_INVALID_WINDOW_HANDLE: i32 = 1400;
        if code == ERROR_INVALID_WINDOW_HANDLE || !unsafe { IsWindow(Some(self.hwnd)).as_bool() } {
            self.destroyed = true;
            return;
        }
        if code == D2DERR_RECREATE_TARGET
            || code == DXGI_ERROR_DEVICE_REMOVED
            || code == DXGI_ERROR_DEVICE_RESET
        {
            log::warn!("D2D device lost ({code:#x}), recreating");
            self.renderer = Renderer::new(self.hwnd, self.renderer.mode);
            reset_devices();
            return;
        }
        self.render_error = Some(code);
        crate::app::post(crate::app::MainMsg::RenderFailed {
            mode: self.renderer.mode,
            code,
        });
    }

    fn hit_item(&self, x: i32, y: i32) -> Option<ItemKey> {
        self.layout.hit(x, y).map(|i| i.key.clone())
    }
}

fn plugin_mouse(key: &Option<ItemKey>, ev: MouseEvent, x: i32, y: i32, hwnd: HWND) -> bool {
    let Some(ItemKey::Plugin(id)) = key else {
        return false;
    };
    state::try_with(|s| {
        s.plugins
            .item(id)
            .map(|i| s.plugins.mouse_event(i, ev, x, y, hwnd.0 as isize))
            .unwrap_or(false)
    })
    .unwrap_or(false)
}

fn lparam_point(lparam: LPARAM) -> (i32, i32) {
    let x = (lparam.0 & 0xFFFF) as u16 as i16 as i32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
    (x, y)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            let need_track = with(|t| {
                let r = !t.tracking;
                t.tracking = true;
                r
            })
            .unwrap_or(false);
            if need_track {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_HOVER | TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: HOVER_TIME_MS,
                };
                unsafe {
                    let _ = TrackMouseEvent(&mut tme);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEHOVER => {
            let popped = with(|t| t.menu_popped).unwrap_or(false);
            let show = state::try_with(|s| s.settings.taskbar.show_tool_tip).unwrap_or(false);
            if show && !popped {
                let mut pt = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut pt);
                }
                crate::app::send_ui(crate::app::UiCommand::ShowTooltip { x: pt.x, y: pt.y });
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with(|t| t.tracking = false);
            crate::app::send_ui(crate::app::UiCommand::HideTooltip);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = lparam_point(lparam);
            let key = with(|t| t.hit_item(x, y)).flatten();
            plugin_mouse(&key, MouseEvent::LClicked, x, y, hwnd);
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = lparam_point(lparam);
            let key = with(|t| t.hit_item(x, y)).flatten();
            if !plugin_mouse(&key, MouseEvent::DbClicked, x, y, hwnd) {
                crate::app::post(crate::app::MainMsg::DoubleClick);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let (x, y) = lparam_point(lparam);
            let key = with(|t| t.hit_item(x, y)).flatten();
            crate::app::send_ui(crate::app::UiCommand::HideTooltip);
            if !plugin_mouse(&key, MouseEvent::RClicked, x, y, hwnd) {
                with(|t| t.menu_popped = true);
                let plugin = match &key {
                    Some(ItemKey::Plugin(id)) => {
                        state::try_with(|s| s.plugins.plugin_of(id)).flatten()
                    }
                    _ => None,
                };
                crate::tray::menu::show_context_menu(hwnd, plugin);
                with(|t| t.menu_popped = false);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16;
            if delta >= 120 || delta <= -120 {
                let (sx, sy) = lparam_point(lparam);
                let mut pt = POINT { x: sx, y: sy };
                unsafe {
                    let _ = ScreenToClient(hwnd, &mut pt);
                }
                let key = with(|t| t.hit_item(pt.x, pt.y)).flatten();
                let ev = if delta > 0 {
                    MouseEvent::WheelUp
                } else {
                    MouseEvent::WheelDown
                };
                if plugin_mouse(&key, ev, pt.x, pt.y, hwnd) {
                    return LRESULT(0);
                }
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_KEYDOWN => {
            let mut pt = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
                let _ = ScreenToClient(hwnd, &mut pt);
            }
            let key = with(|t| t.hit_item(pt.x, pt.y)).flatten();
            if let Some(ItemKey::Plugin(id)) = key {
                let (ctrl, shift, alt) = unsafe {
                    (
                        GetKeyState(VK_CONTROL.0 as i32) < 0,
                        GetKeyState(VK_SHIFT.0 as i32) < 0,
                        GetKeyState(VK_MENU.0 as i32) < 0,
                    )
                };
                let handled = state::try_with(|s| {
                    s.plugins
                        .item(&id)
                        .map(|i| {
                            s.plugins.key_event(
                                i,
                                wparam.0 as i32,
                                ctrl,
                                shift,
                                alt,
                                hwnd.0 as isize,
                            )
                        })
                        .unwrap_or(false)
                })
                .unwrap_or(false);
                if handled {
                    return LRESULT(0);
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe {
                BeginPaint(hwnd, &mut ps);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_EXITMENULOOP => {
            with(|t| t.menu_popped = false);
            LRESULT(0)
        }
        WM_TABLET_QUERYSYSTEMGESTURESTATUS => LRESULT(0),
        WM_COMMAND => {
            crate::tray::menu::on_command(hwnd, (wparam.0 & 0xFFFF) as u32);
            LRESULT(0)
        }
        WM_DESTROY => {
            TASKBAR.with(|t| {
                if let Ok(mut t) = t.try_borrow_mut() {
                    if let Some(t) = t.as_mut() {
                        if t.hwnd == hwnd {
                            t.destroyed = true;
                        }
                    }
                }
            });
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
