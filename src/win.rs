use std::ffi::c_void;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, RECT};
use windows::Win32::Globalization::{CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar};
use windows::Win32::Graphics::Dwm::DwmGetColorizationColor;
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromRect};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_DWORD, RegCloseKey, RegGetValueW, RegOpenKeyExW,
    RegQueryValueExW,
};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::{
    GetCurrentProcess, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, OpenProcessToken,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling, SetPriorityClass,
    SetProcessInformation,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MESSAGEBOX_STYLE, MessageBoxW, SW_SHOWNORMAL,
};
use windows::core::{PCWSTR, w};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// # Safety
/// `p` must be null or point to a NUL terminated UTF-16 string.
pub unsafe fn from_pcwstr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    unsafe {
        while *p.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
    }
}

pub fn ansi_to_string(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    unsafe {
        let n = MultiByteToWideChar(CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, None);
        if n <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut buf = vec![0u16; n as usize];
        MultiByteToWideChar(
            CP_ACP,
            MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0),
            bytes,
            Some(&mut buf),
        );
        String::from_utf16_lossy(&buf)
    }
}

pub fn user_ui_language() -> String {
    unsafe {
        let mut buf = [0u16; 85];
        let n = windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buf);
        if n > 0 {
            return from_wide(&buf);
        }
    }
    String::from("en-US")
}

pub fn reg_dword(root: HKEY, path: &str, name: &str) -> Option<u32> {
    let path = wide(path);
    let name = wide(name);
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let r = unsafe {
        RegGetValueW(
            root,
            PCWSTR(path.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    if r.is_ok() { Some(data) } else { None }
}

pub fn reg_string(root: HKEY, path: &str, name: &str) -> Option<String> {
    unsafe {
        let mut key = HKEY::default();
        let p = wide(path);
        if RegOpenKeyExW(root, PCWSTR(p.as_ptr()), Some(0), KEY_READ, &mut key).is_err() {
            return None;
        }
        let n = wide(name);
        let mut buf = vec![0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let r = RegQueryValueExW(
            key,
            PCWSTR(n.as_ptr()),
            None,
            None,
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        if r.is_ok() {
            Some(from_wide(&buf))
        } else {
            None
        }
    }
}

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const EXPLORER_ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";

/// Same key as `CWindowsSettingHelper::CheckWindows10LightTheme`: drives the taskbar / tray colors.
pub fn system_uses_light_theme() -> bool {
    windows_version().major >= 10
        && reg_dword(HKEY_CURRENT_USER, PERSONALIZE, "SystemUsesLightTheme").unwrap_or(0) != 0
}

pub fn apps_use_light_theme() -> bool {
    reg_dword(HKEY_CURRENT_USER, PERSONALIZE, "AppsUseLightTheme").unwrap_or(1) != 0
}

pub fn is_taskbar_center_align() -> bool {
    reg_dword(HKEY_CURRENT_USER, EXPLORER_ADVANCED, "TaskbarAl")
        .map(|v| v != 0)
        .unwrap_or(true)
}

pub fn is_taskbar_widgets_btn_shown(web_experience_detected: bool) -> bool {
    if !web_experience_detected {
        return false;
    }
    reg_dword(HKEY_CURRENT_USER, EXPLORER_ADVANCED, "TaskbarDa")
        .map(|v| v != 0)
        .unwrap_or(true)
}

pub fn is_taskbar_showing_in_all_displays() -> bool {
    reg_dword(HKEY_CURRENT_USER, EXPLORER_ADVANCED, "MMTaskbarEnabled")
        .map(|v| v != 0)
        .unwrap_or(true)
}

/// Mirrors `WindowsWebExperienceDetector`: the widgets package registers this AppX family.
pub fn is_windows_web_experience_detected() -> bool {
    let base = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Families";
    unsafe {
        let mut key = HKEY::default();
        let p = wide(base);
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(p.as_ptr()),
            Some(0),
            KEY_READ,
            &mut key,
        )
        .is_err()
        {
            return false;
        }
        let mut found = false;
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 512];
            let mut len = name.len() as u32;
            let r = windows::Win32::System::Registry::RegEnumKeyExW(
                key,
                index,
                Some(windows::core::PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            );
            if r.is_err() {
                break;
            }
            if from_wide(&name).starts_with("MicrosoftWindows.Client.WebExperience") {
                found = true;
                break;
            }
            index += 1;
        }
        let _ = RegCloseKey(key);
        found
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WinVersion {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
}

impl WinVersion {
    pub fn is_windows11_or_later(&self) -> bool {
        self.major > 10 || (self.major == 10 && self.build >= 22000)
    }
    pub fn is_windows10_or_later(&self) -> bool {
        self.major >= 10
    }
    pub fn is_windows8_or_81(&self) -> bool {
        self.major == 6 && (self.minor == 2 || self.minor == 3)
    }
    pub fn is_windows7(&self) -> bool {
        self.major == 6 && self.minor == 1
    }
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetVersion(info: *mut OSVERSIONINFOW) -> i32;
}

pub fn windows_version() -> WinVersion {
    use std::sync::OnceLock;
    static V: OnceLock<WinVersion> = OnceLock::new();
    *V.get_or_init(|| unsafe {
        let mut info = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        RtlGetVersion(&mut info);
        WinVersion {
            major: info.dwMajorVersion,
            minor: info.dwMinorVersion,
            build: info.dwBuildNumber,
        }
    })
}

/// Task Manager efficiency mode: EcoQoS plus idle priority. Windows 11 only; older systems stay unchanged.
pub fn set_efficiency_mode(enable: bool) {
    if !windows_version().is_windows11_or_later() {
        return;
    }
    unsafe {
        let process = GetCurrentProcess();
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: if enable {
                PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            } else {
                0
            },
        };
        if let Err(e) = SetProcessInformation(
            process,
            ProcessPowerThrottling,
            &state as *const PROCESS_POWER_THROTTLING_STATE as *const c_void,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        ) {
            log::warn!("SetProcessInformation(ProcessPowerThrottling) failed: {e}");
        }
        let priority = if enable {
            IDLE_PRIORITY_CLASS
        } else {
            NORMAL_PRIORITY_CLASS
        };
        if let Err(e) = SetPriorityClass(process, priority) {
            log::warn!("SetPriorityClass failed: {e}");
        }
    }
}

pub fn is_elevated() -> bool {
    use std::sync::OnceLock;
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(|| unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    })
}

pub fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

pub fn exe_dir() -> PathBuf {
    exe_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default()
}

/// Velopack install root (the directory that contains `Update.exe`), when this process is an
/// installed copy. Portable zip and `cargo` builds return `None`.
///
/// The locator only recognizes `...\current\TrafficBar.exe`. After a junction is resolved the
/// path is `...\app-<version>\TrafficBar.exe`, so also walk up from the exe until `Update.exe`
/// appears.
pub fn install_root() -> Option<PathBuf> {
    use velopack::locator::{LocationContext, VelopackLocator};
    if let Ok(loc) = VelopackLocator::try_from(LocationContext::FromCurrentExe) {
        let root = loc.get_root_dir();
        if root.join("Update.exe").is_file() {
            return Some(root);
        }
    }
    let mut dir = exe_dir();
    for _ in 0..5 {
        if dir.join("Update.exe").is_file() && dir.join("current").is_dir() {
            return Some(dir);
        }
        dir = dir.parent().map(Path::to_path_buf)?;
    }
    None
}

/// Portable zip and unpacked/`cargo` builds have no `Update.exe`. An installed copy does.
pub fn is_portable_edition() -> bool {
    install_root().is_none()
}

/// Directory for `global_cfg.ini`. Under a Velopack install this is the pack root (sibling of
/// `current`). In-app updates replace `current` and leave this directory alone. Setup.exe repair
/// does not: it renames the whole root and deletes it.
pub fn program_dir() -> PathBuf {
    install_root().unwrap_or_else(exe_dir)
}

/// Where plugin dlls are loaded from.
///
/// An installed copy keeps them in `%AppData%\TrafficBar\plugins`. Setup.exe repair and uninstall
/// delete `%LocalAppData%\TrafficBar` (everything beside `Update.exe`, including a `plugins`
/// folder stored there). Roaming AppData is outside that tree.
///
/// A portable zip has no `Update.exe` and still uses `plugins` next to the exe.
pub fn plugin_dir() -> PathBuf {
    if install_root().is_some() {
        appdata_dir().join("plugins")
    } else {
        exe_dir().join("plugins")
    }
}

pub fn appdata_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(exe_dir)
        .join("TrafficBar")
}

pub fn system_dir() -> PathBuf {
    unsafe {
        let mut buf = [0u16; 260];
        let n = windows::Win32::System::SystemInformation::GetSystemDirectoryW(Some(&mut buf));
        PathBuf::from(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

/// Restarts the current executable with the `runas` verb. Returns true if the elevated process started.
pub fn restart_as_admin() -> bool {
    relaunch(w!("runas"), "--restarted")
}

/// Restarts the current executable in place. An elevated process stays elevated, without a UAC prompt.
pub fn restart() -> bool {
    relaunch(w!("open"), "--restarted")
}

/// Asks UAC to start an elevated copy. Returns false when the user declines or the launch fails.
/// `--elevate-attempted` keeps that copy from asking again if it is still not elevated.
pub fn elevate_at_startup() -> bool {
    relaunch(w!("runas"), "--restarted --elevate-attempted")
}

fn relaunch(verb: windows::core::PCWSTR, args: &str) -> bool {
    let exe = wide(&exe_path().to_string_lossy());
    let args = wide(args);
    let r = unsafe {
        ShellExecuteW(
            None,
            verb,
            PCWSTR(exe.as_ptr()),
            PCWSTR(args.as_ptr()),
            None,
            SW_SHOWNORMAL,
        )
    };
    r.0 as usize > 32
}

/// Opens a file, folder, or URL with the shell.
///
/// `ShellExecuteW` pumps window messages. Calling it on the UI thread from a GPUI handler
/// reenters the window procedure while `App` is still borrowed and panics (`RefCell already borrowed`).
pub fn shell_open(target: &str) {
    if target.is_empty() {
        return;
    }
    let target = target.to_owned();
    std::thread::spawn(move || {
        let t = wide(&target);
        unsafe {
            let initialized =
                CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).is_ok();
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(t.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            );
            if initialized {
                CoUninitialize();
            }
        }
    });
}

pub fn message_box(owner: Option<HWND>, text: &str, warning: bool) {
    let t = wide(text);
    let caption = wide("TrafficBar");
    let style: MESSAGEBOX_STYLE = MB_OK
        | if warning {
            MB_ICONWARNING
        } else {
            MB_ICONINFORMATION
        };
    unsafe {
        MessageBoxW(owner, PCWSTR(t.as_ptr()), PCWSTR(caption.as_ptr()), style);
    }
}

pub fn dpi_from_rect(rect: &RECT) -> Option<u32> {
    unsafe {
        let mon = MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST);
        let mut x = 0u32;
        let mut y = 0u32;
        GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut x, &mut y)
            .ok()
            .map(|_| x)
    }
}

/// System accent color as 0x00BBGGRR (COLORREF), like `OnColorizationColorChanged` in the original.
pub fn theme_color() -> u32 {
    unsafe {
        let mut color = 0u32;
        let mut opaque = windows::core::BOOL(0);
        if DwmGetColorizationColor(&mut color, &mut opaque).is_ok() {
            let r = (color >> 16) & 0xFF;
            let g = (color >> 8) & 0xFF;
            let b = color & 0xFF;
            return r | (g << 8) | (b << 16);
        }
    }
    0x00D77800
}
