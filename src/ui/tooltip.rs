//! Mouse tooltip of the taskbar window (`CTaskBarDlg::GetMouseTipsInfo`), as a borderless,
//! non-activating, click-through GPUI popup.

use std::cell::RefCell;

use gpui::{
    App, AppContext, Bounds, Context, IntoElement, ParentElement, Render, Styled, Window, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowKind, WindowOptions, div, point, px, size,
};
use gpui_component::{ActiveTheme, Root, v_flex};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::display_item::{DisplayItem, HardwareItem, ItemKey};
use crate::format;
use crate::i18n::tr;
use crate::state;

const FONT_PX: f32 = 12.0;
const LINE_PX: f32 = 18.0;
const PAD_PX: f32 = 8.0;

struct Tip {
    handle: WindowHandle<Root>,
    view: gpui::Entity<TooltipView>,
    hwnd: isize,
    visible: bool,
    anchor: (i32, i32),
    lines: Vec<String>,
}

thread_local! {
    static TIP: RefCell<Option<Tip>> = const { RefCell::new(None) };
}

pub struct TooltipView {
    lines: Vec<String>,
}

impl Render for TooltipView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .size_full()
            .px(px(PAD_PX))
            .py(px(PAD_PX / 2.0))
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .text_size(px(FONT_PX))
            .children(self.lines.iter().map(|l| div().h(px(LINE_PX)).line_height(px(LINE_PX)).child(l.clone())))
    }
}

/// Text of the tooltip, one entry per line.
pub fn tip_lines() -> Vec<String> {
    state::try_with(|s| {
        let t = &s.settings.taskbar;
        let snap = &s.snapshot;
        let shown = |d: DisplayItem| t.display_item.contains(d);
        let mut lines = Vec::new();
        lines.push(format!(
            "{}: {}",
            tr("IDS_TRAFFIC_USED_TODAY"),
            format::kbytes_to_string((snap.today_up_traffic + snap.today_down_traffic) / 1024)
        ));
        lines.push(format!(
            " ({}: {}/{}: {})",
            tr("IDS_UPLOAD"),
            format::kbytes_to_string(snap.today_up_traffic / 1024),
            tr("IDS_DOWNLOAD"),
            format::kbytes_to_string(snap.today_down_traffic / 1024)
        ));
        if !shown(DisplayItem::Up) {
            lines.push(format!("{}: {}/s", tr("IDS_UPLOAD"), format::speed_to_string(snap.out_speed, t)));
        }
        if !shown(DisplayItem::Down) {
            lines.push(format!("{}: {}/s", tr("IDS_DOWNLOAD"), format::speed_to_string(snap.in_speed, t)));
        }
        if !shown(DisplayItem::Cpu) {
            lines.push(format!("{}: {} %", tr("IDS_CPU_USAGE"), snap.cpu_usage));
        }
        let used = format::kbytes_to_string(snap.used_memory_kb);
        let total = format::kbytes_to_string(snap.total_memory_kb);
        if !shown(DisplayItem::Memory) {
            lines.push(format!("{}: {}/{} ({} %)", tr("IDS_MEMORY_USAGE"), used, total, snap.memory_usage));
        } else {
            lines.push(format!("{}: {}/{}", tr("IDS_MEMORY_USAGE"), used, total));
        }
        if !shown(DisplayItem::CpuFreq) && snap.cpu_freq > 0.0 {
            lines.push(format!("{}: {}", tr("IDS_CPU_FREQ"), format::freq_to_string(snap.cpu_freq, t)));
        }
        if !shown(DisplayItem::GpuUsage) && snap.gpu_usage >= 0 {
            lines.push(format!("{}: {} %", tr("IDS_GPU_USAGE"), snap.gpu_usage));
        }
        let g = &s.settings.general;
        if s.rt.elevated && g.any_hardware_enabled() {
            let temps = [
                (DisplayItem::CpuTemp, HardwareItem::Cpu, snap.cpu_temperature, "IDS_CPU_TEMPERATURE"),
                (DisplayItem::GpuTemp, HardwareItem::Gpu, snap.gpu_temperature, "IDS_GPU_TEMPERATURE"),
                (DisplayItem::HddTemp, HardwareItem::Hdd, snap.hdd_temperature, "IDS_HDD_TEMPERATURE"),
                (DisplayItem::MainBoardTemp, HardwareItem::MainBoard, snap.main_board_temperature, "IDS_MAINBOARD_TEMPERATURE"),
            ];
            for (item, hw, value, key) in temps {
                if g.is_hardware_enabled(hw) && !shown(item) && value > 0.0 {
                    lines.push(format!("{}: {}", tr(key), format::temperature_to_string(value, t)));
                }
            }
        }
        if !shown(DisplayItem::HddUsage) && snap.hdd_usage >= 0 {
            lines.push(format!("{}: {} %", tr("IDS_HDD_USAGE"), snap.hdd_usage));
        }
        let admin_items: Vec<String> = crate::display_item::ALL_BUILTIN
            .iter()
            .filter(|d| d.requires_admin() && !s.rt.elevated && t.is_item_displayed(&ItemKey::Builtin(**d)))
            .map(|d| d.name())
            .collect();
        if !admin_items.is_empty() {
            lines.push(format!("{}: {}", admin_items.join(", "), tr("BAR_ADMIN_REQUIRED")));
        }
        for extra in s.plugins.tooltip_info().split('\n').filter(|l| !l.trim().is_empty()) {
            lines.push(extra.trim_end().to_string());
        }
        lines
    })
    .unwrap_or_default()
}

/// Creates the hidden tooltip window. GPUI quits once its last window closes, so this window also
/// keeps the app alive while no dialog is open.
pub fn ensure_window(cx: &mut App) -> bool {
    if TIP.with(|t| t.borrow().is_some()) {
        return true;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(-2000.), px(-2000.)), size(px(200.), px(60.))))),
        titlebar: None,
        focus: false,
        show: false,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    };
    let mut view_slot = None;
    let result = cx.open_window(options, |window, cx| {
        let view = cx.new(|_| TooltipView { lines: Vec::new() });
        view_slot = Some(view.clone());
        cx.new(|cx| Root::new(view, window, cx))
    });
    let (Ok(handle), Some(view)) = (result, view_slot) else {
        return false;
    };
    let hwnd = handle.update(cx, |_, window, _| super::hwnd_of(window)).unwrap_or(0);
    unsafe {
        let h = HWND(hwnd as *mut _);
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        SetWindowLongPtrW(
            h,
            GWL_EXSTYLE,
            ex | (WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST).0 as isize,
        );
    }
    TIP.with(|t| {
        *t.borrow_mut() = Some(Tip {
            handle,
            view,
            hwnd,
            visible: false,
            anchor: (0, 0),
            lines: Vec::new(),
        })
    });
    true
}

fn place(hwnd: HWND, anchor: (i32, i32), lines: &[String]) {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32;
    let scale = dpi / 96.0;
    let font = crate::i18n::default_font();
    let widths = crate::taskbar::measure_lines(&font, FONT_PX * scale, lines);
    let text_w = widths.into_iter().max().unwrap_or(100) as f32;
    let w = (text_w + (PAD_PX * 2.0 + 10.0) * scale).ceil() as i32;
    let h = ((lines.len().max(1) as f32 * LINE_PX + PAD_PX + 2.0) * scale).ceil() as i32;
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let mon = MonitorFromPoint(POINT { x: anchor.0, y: anchor.1 }, MONITOR_DEFAULTTONEAREST);
        let _ = GetMonitorInfoW(mon, &mut mi);
    }
    let area: RECT = mi.rcMonitor;
    let mut x = anchor.0;
    let mut y = anchor.1 + (20.0 * scale) as i32;
    if y + h > area.bottom {
        y = anchor.1 - h - (4.0 * scale) as i32;
    }
    if x + w > area.right {
        x = area.right - w;
    }
    x = x.max(area.left);
    y = y.max(area.top);
    unsafe {
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

pub fn show(x: i32, y: i32, cx: &mut App) {
    if !ensure_window(cx) {
        return;
    }
    let lines = tip_lines();
    let info = TIP.with(|t| {
        let mut t = t.borrow_mut();
        let tip = t.as_mut()?;
        tip.visible = true;
        tip.anchor = (x, y);
        tip.lines = lines.clone();
        Some((tip.view.clone(), tip.hwnd))
    });
    let Some((view, hwnd)) = info else { return };
    view.update(cx, |v, cx| {
        v.lines = lines.clone();
        cx.notify();
    });
    place(HWND(hwnd as *mut _), (x, y), &lines);
}

pub fn refresh(cx: &mut App) {
    let info = TIP.with(|t| {
        let t = t.borrow();
        let tip = t.as_ref()?;
        tip.visible.then(|| (tip.view.clone(), tip.hwnd, tip.anchor, tip.lines.clone()))
    });
    let Some((view, hwnd, anchor, old)) = info else { return };
    let lines = tip_lines();
    if lines != old {
        let resize = lines.len() != old.len()
            || lines.iter().map(|l| l.chars().count()).max() != old.iter().map(|l| l.chars().count()).max();
        TIP.with(|t| {
            if let Some(tip) = t.borrow_mut().as_mut() {
                tip.lines = lines.clone();
            }
        });
        view.update(cx, |v, cx| {
            v.lines = lines.clone();
            cx.notify();
        });
        if resize {
            place(HWND(hwnd as *mut _), anchor, &lines);
        }
    }
}

pub fn hide() {
    TIP.with(|t| {
        if let Some(tip) = t.borrow_mut().as_mut() {
            if tip.visible {
                tip.visible = false;
                unsafe {
                    let _ = ShowWindow(HWND(tip.hwnd as *mut _), SW_HIDE);
                }
            }
        }
    });
}

pub fn destroy(cx: &mut App) {
    if let Some(tip) = TIP.with(|t| t.borrow_mut().take()) {
        let _ = tip.handle.update(cx, |_, window, _| window.remove_window());
    }
}
