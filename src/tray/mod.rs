//! Notification area icon (Shell_NotifyIcon) and the native Win32 context menu.

pub mod menu;

use std::cell::RefCell;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SM_CXSMICON,
};
use windows::core::PCWSTR;

use crate::format;
use crate::i18n::tr;

pub const WM_APP_TRAY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;
pub const IDI_NOTIFY_DARK: u16 = 101;
pub const IDI_NOTIFY_LIGHT: u16 = 102;
const TRAY_ID: u32 = 1;

struct Tray {
    hwnd: HWND,
    icon: HICON,
    light: bool,
    added: bool,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

/// Loads "图标 1" for the given taskbar theme: the white icon for dark taskbars and the black
/// one for light taskbars (`AutoSelectNotifyIcon`).
pub fn load_notify_icon(light: bool, size: i32) -> HICON {
    let id = if light { IDI_NOTIFY_LIGHT } else { IDI_NOTIFY_DARK };
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        LoadImageW(Some(hinst.into()), PCWSTR(id as usize as *const u16), IMAGE_ICON, size, size, LR_DEFAULTCOLOR)
            .map(|h| HICON(h.0))
            .unwrap_or_default()
    }
}

fn small_icon_size() -> i32 {
    unsafe {
        let dpi = GetDpiForSystem();
        GetSystemMetricsForDpi(SM_CXSMICON, dpi)
    }
}

fn copy_wide(dst: &mut [u16], s: &str) {
    let w: Vec<u16> = s.encode_utf16().collect();
    let n = w.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&w[..n]);
    dst[n] = 0;
}

fn base_data(t: &Tray) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: t.hwnd,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: WM_APP_TRAY,
        hIcon: t.icon,
        ..Default::default()
    }
}

pub fn create(hwnd: HWND, light: bool) {
    let icon = load_notify_icon(light, small_icon_size());
    let mut t = Tray {
        hwnd,
        icon,
        light,
        added: false,
    };
    let mut data = base_data(&t);
    copy_wide(&mut data.szTip, &tr("APP_NAME"));
    t.added = unsafe { Shell_NotifyIconW(NIM_ADD, &data).as_bool() };
    if !t.added {
        log::warn!("Shell_NotifyIcon(NIM_ADD) failed");
    }
    TRAY.with(|c| *c.borrow_mut() = Some(t));
}

/// Re-adds the icon after explorer restarts (`TaskbarCreated`).
pub fn readd() {
    TRAY.with(|c| {
        if let Some(t) = c.borrow_mut().as_mut() {
            let data = base_data(t);
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &data);
            }
            t.added = unsafe { Shell_NotifyIconW(NIM_ADD, &data).as_bool() };
        }
    });
}

pub fn set_theme(light: bool) {
    TRAY.with(|c| {
        if let Some(t) = c.borrow_mut().as_mut() {
            if t.light == light {
                return;
            }
            let old = t.icon;
            t.icon = load_notify_icon(light, small_icon_size());
            t.light = light;
            let data = base_data(t);
            unsafe {
                let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
                if !old.is_invalid() {
                    let _ = DestroyIcon(old);
                }
            }
        }
    });
}

/// `UpdateNotifyIconTip`.
pub fn update_tip() {
    let tip = crate::state::try_with(|s| {
        let snap = &s.snapshot;
        let g = &s.settings.general;
        let mut tip = tr("APP_NAME");
        tip += &format!("\n{}: {}/s", tr("IDS_UPLOAD"), format::data_size_to_string(snap.out_speed, true));
        tip += &format!("\n{}: {}/s", tr("IDS_DOWNLOAD"), format::data_size_to_string(snap.in_speed, true));
        tip += &format!("\nCPU: {} %", snap.cpu_usage);
        tip += &format!("\n{}: {} %", tr("IDS_MEMORY"), snap.memory_usage);
        if s.rt.elevated && g.any_hardware_enabled() {
            use crate::display_item::HardwareItem::*;
            if g.is_hardware_enabled(Gpu) && snap.gpu_usage >= 0 {
                tip += &format!("\n{}: {} %", tr("IDS_GPU_USAGE"), snap.gpu_usage);
            }
            if g.is_hardware_enabled(Cpu) && snap.cpu_temperature > 0.0 {
                tip += &format!("\n{}: {} °C", tr("IDS_CPU_TEMPERATURE"), snap.cpu_temperature as i32);
            }
            if g.is_hardware_enabled(Gpu) && snap.gpu_temperature > 0.0 {
                tip += &format!("\n{}: {} °C", tr("IDS_GPU_TEMPERATURE"), snap.gpu_temperature as i32);
            }
            if g.is_hardware_enabled(Hdd) && snap.hdd_temperature > 0.0 {
                tip += &format!("\n{}: {} °C", tr("IDS_HDD_TEMPERATURE"), snap.hdd_temperature as i32);
            }
            if g.is_hardware_enabled(MainBoard) && snap.main_board_temperature > 0.0 {
                tip += &format!("\n{}: {} °C", tr("IDS_MAINBOARD_TEMPERATURE"), snap.main_board_temperature as i32);
            }
        }
        tip
    });
    let Some(tip) = tip else { return };
    TRAY.with(|c| {
        if let Some(t) = c.borrow().as_ref() {
            let mut data = base_data(t);
            copy_wide(&mut data.szTip, &tip);
            unsafe {
                let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
            }
        }
    });
}

/// Balloon notification (`ShowNotifyTip`).
pub fn notify(title: &str, message: &str) {
    TRAY.with(|c| {
        if let Some(t) = c.borrow().as_ref() {
            let mut data = base_data(t);
            data.uFlags |= NIF_INFO;
            data.dwInfoFlags = NIIF_INFO;
            copy_wide(&mut data.szInfoTitle, title);
            copy_wide(&mut data.szInfo, message);
            unsafe {
                let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
            }
        }
    });
}

pub fn destroy() {
    TRAY.with(|c| {
        if let Some(t) = c.borrow_mut().take() {
            let data = base_data(&t);
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &data);
                if !t.icon.is_invalid() {
                    let _ = DestroyIcon(t.icon);
                }
            }
        }
    });
}
