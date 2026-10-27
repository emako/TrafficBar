//! Built-in memory cleaning. Nothing new is drawn on the taskbar: the user releases
//! caches from the context menu, by clicking the memory label, or when usage stays
//! above a threshold.
//!
//! The system calls follow Mem Reduct by Henry++ (GPL-3.0).

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_MORE_DATA, GetLastError, HANDLE, LUID,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_INCREASE_QUOTA_NAME,
    SE_PRIVILEGE_ENABLED, SE_PROF_SINGLE_PROCESS_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
    TOKEN_QUERY,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_WRITE_DATA, FlushFileBuffers, OPEN_EXISTING, SYNCHRONIZE,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::PCWSTR;

use crate::config::MemoryCleanTrigger;
use crate::display_item::{DisplayItem, ItemKey};
use crate::i18n::tr;
use crate::state;
use crate::win;

pub const WORKING_SET: u32 = 0x01;
pub const SYSTEM_FILE_CACHE: u32 = 0x02;
pub const STANDBY_PRIORITY0: u32 = 0x04;
pub const STANDBY_LIST: u32 = 0x08;
pub const MODIFIED_PAGE_LIST: u32 = 0x10;
pub const COMBINE_MEMORY_LISTS: u32 = 0x20;
pub const REGISTRY_CACHE: u32 = 0x40;
pub const MODIFIED_FILE_CACHE: u32 = 0x80;

pub const MASK_ALL: u32 = WORKING_SET
    | SYSTEM_FILE_CACHE
    | STANDBY_PRIORITY0
    | STANDBY_LIST
    | MODIFIED_PAGE_LIST
    | COMBINE_MEMORY_LISTS
    | REGISTRY_CACHE
    | MODIFIED_FILE_CACHE;

/// Same regions as Mem Reduct `REDUCT_MASK_DEFAULT` (231 / 0xE7).
pub const MASK_DEFAULT: u32 = WORKING_SET
    | SYSTEM_FILE_CACHE
    | STANDBY_PRIORITY0
    | REGISTRY_CACHE
    | COMBINE_MEMORY_LISTS
    | MODIFIED_FILE_CACHE;

const SYSTEM_MEMORY_LIST_INFORMATION: u32 = 80;
const SYSTEM_FILE_CACHE_INFORMATION_EX: u32 = 81;
const SYSTEM_COMBINE_PHYSICAL_MEMORY_INFORMATION: u32 = 130;
const SYSTEM_REGISTRY_RECONCILIATION_INFORMATION: u32 = 155;

const MEMORY_EMPTY_WORKING_SETS: u32 = 2;
const MEMORY_FLUSH_MODIFIED_LIST: u32 = 3;
const MEMORY_PURGE_STANDBY_LIST: u32 = 4;
const MEMORY_PURGE_LOW_PRIORITY_STANDBY_LIST: u32 = 5;

/// `IOCTL_MOUNTMGR_QUERY_POINTS`.
const MOUNTMGR_QUERY_POINTS: u32 = 0x006D_0008;

/// While usage stays above the threshold, automatic release waits this long between runs.
pub const AUTO_COOLDOWN_SEC: u64 = 60;

static BUSY: AtomicBool = AtomicBool::new(false);
static WARNED_PRIVILEGE: AtomicBool = AtomicBool::new(false);
/// The button-up that follows a double-click must not release a second time.
static IGNORE_UP: AtomicBool = AtomicBool::new(false);

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSetSystemInformation(class: u32, info: *mut c_void, length: u32) -> i32;
}

struct BusyGuard;

impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

enum CleanStatus {
    Ok,
    NotElevated,
}

struct CleanResult {
    status: CleanStatus,
    released_bytes: u64,
}

pub fn normalize_mask(mask: u32) -> u32 {
    let mask = mask & MASK_ALL;
    if mask == 0 { MASK_DEFAULT } else { mask }
}

/// Starts a clean. `interactive` shows the "already running" and privilege notices
/// that an automatic pass stays quiet about. Returns whether a clean was started.
pub fn release(interactive: bool) -> bool {
    if BUSY.load(Ordering::Acquire) {
        if interactive {
            notify(&tr("TXT_MEMORY_CLEAN_BUSY"));
        }
        return false;
    }
    if !win::is_elevated() {
        if interactive || !WARNED_PRIVILEGE.swap(true, Ordering::Relaxed) {
            notify(&tr("TXT_MEMORY_CLEAN_NO_PRIVILEGES"));
        }
        return false;
    }
    if BUSY.swap(true, Ordering::AcqRel) {
        if interactive {
            notify(&tr("TXT_MEMORY_CLEAN_BUSY"));
        }
        return false;
    }

    let (mask, notify_result) = state::try_with(|s| {
        (
            normalize_mask(s.settings.general.memory_clean_mask),
            s.settings.general.memory_clean_notify,
        )
    })
    .unwrap_or((MASK_DEFAULT, true));

    let guard = BusyGuard;
    let spawned = thread::Builder::new()
        .name("memory-clean".into())
        .spawn(move || {
            let _guard = guard;
            let result = run(mask);
            match result.status {
                CleanStatus::NotElevated => notify(&tr("TXT_MEMORY_CLEAN_NO_PRIVILEGES")),
                CleanStatus::Ok if notify_result => {
                    let size = format_bytes(result.released_bytes);
                    notify(&tr("TXT_MEMORY_CLEAN_DONE").replace("%s", &size));
                }
                CleanStatus::Ok => {}
            }
        });
    match spawned {
        Ok(_) => true,
        Err(e) => {
            log::warn!("memory clean thread failed: {e}");
            BUSY.store(false, Ordering::Release);
            if interactive {
                notify(&tr("TXT_MEMORY_CLEAN_UNAVAILABLE"));
            }
            false
        }
    }
}

/// Taskbar click on the memory label. Returns true when the click is consumed,
/// so a double-click does not also run the global double-click action.
pub fn on_taskbar_mouse(key: &Option<ItemKey>, double: bool) -> bool {
    let memory = matches!(key, Some(ItemKey::Builtin(DisplayItem::Memory)));
    let mode = state::try_with(|s| s.settings.general.memory_clean_trigger)
        .unwrap_or(MemoryCleanTrigger::DoubleClick);
    if double {
        if !memory {
            return false;
        }
        return match mode {
            MemoryCleanTrigger::Click => {
                IGNORE_UP.store(true, Ordering::Relaxed);
                true
            }
            MemoryCleanTrigger::DoubleClick => {
                release(true);
                true
            }
            MemoryCleanTrigger::Threshold | MemoryCleanTrigger::None => false,
        };
    }
    if IGNORE_UP.swap(false, Ordering::Relaxed) {
        return memory && mode == MemoryCleanTrigger::Click;
    }
    if memory && mode == MemoryCleanTrigger::Click {
        release(true);
        return true;
    }
    false
}

fn notify(message: &str) {
    crate::app::post_notify(tr("TXT_RELEASE_MEMORY"), message.to_string());
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < 4 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

fn windows_at_least(major: u32, minor: u32) -> bool {
    let v = win::windows_version();
    v.major > major || (v.major == major && v.minor >= minor)
}

fn enable_privilege(name: PCWSTR) {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .is_err()
        {
            return;
        }
        let mut luid = LUID::default();
        if LookupPrivilegeValueW(None, name, &mut luid).is_ok() {
            let privileges = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            let _ = AdjustTokenPrivileges(token, false, Some(&privileges), 0, None, None);
        }
        let _ = CloseHandle(token);
    }
}

fn used_physical_bytes() -> u64 {
    unsafe {
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if GlobalMemoryStatusEx(&mut status).is_err() || status.ullTotalPhys < status.ullAvailPhys
        {
            return 0;
        }
        status.ullTotalPhys - status.ullAvailPhys
    }
}

fn set_info(class: u32, data: *mut c_void, len: u32) {
    unsafe {
        let _ = NtSetSystemInformation(class, data, len);
    }
}

#[repr(C)]
struct FileCacheInformation {
    current_size: usize,
    peak_size: usize,
    page_fault_count: u32,
    minimum_working_set: usize,
    maximum_working_set: usize,
    current_size_including_transition: usize,
    peak_size_including_transition: usize,
    transition_repurpose_count: u32,
    flags: u32,
}

#[repr(C)]
struct MemoryCombineInformationEx {
    handle: HANDLE,
    pages_combined: usize,
    flags: u32,
}

#[repr(C)]
struct MountPoint {
    symbolic_link_name_offset: u32,
    symbolic_link_name_length: u16,
    reserved1: u16,
    unique_id_offset: u32,
    unique_id_length: u16,
    reserved2: u16,
    device_name_offset: u32,
    device_name_length: u16,
    reserved3: u16,
}

fn read_u32(buf: &[u8], off: usize) -> Option<u32> {
    let b = buf.get(off..off + 4)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}

fn read_u16(buf: &[u8], off: usize) -> Option<u16> {
    let b = buf.get(off..off + 2)?;
    Some(u16::from_le_bytes(b.try_into().ok()?))
}

fn is_volume_name(name: &[u16]) -> bool {
    let n = name.len();
    if !(n == 48 || (n == 49 && name[48] == '\\' as u16)) {
        return false;
    }
    let expect = [
        (0, '\\'),
        (2, '?'),
        (3, '\\'),
        (4, 'V'),
        (5, 'o'),
        (6, 'l'),
        (7, 'u'),
        (8, 'm'),
        (9, 'e'),
        (10, '{'),
        (19, '-'),
        (24, '-'),
        (29, '-'),
        (34, '-'),
        (47, '}'),
    ];
    if name[1] != '?' as u16 && name[1] != '\\' as u16 {
        return false;
    }
    expect
        .iter()
        .all(|(i, ch)| name[*i] == *ch as u16)
}

fn flush_modified_file_cache() {
    let device_name = win::wide("\\\\.\\MountPointManager");
    let device = unsafe {
        CreateFileW(
            PCWSTR(device_name.as_ptr()),
            (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    };
    let Ok(device) = device else {
        return;
    };

    let query = MountPoint {
        symbolic_link_name_offset: 0,
        symbolic_link_name_length: 0,
        reserved1: 0,
        unique_id_offset: 0,
        unique_id_length: 0,
        reserved2: 0,
        device_name_offset: 0,
        device_name_length: 0,
        reserved3: 0,
    };
    let mut buffer = vec![0u8; std::mem::size_of::<MountPoint>() * 8 + 8];
    let mut queried = false;
    for _ in 0..6 {
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                device,
                MOUNTMGR_QUERY_POINTS,
                Some(&query as *const MountPoint as *const c_void),
                std::mem::size_of::<MountPoint>() as u32,
                Some(buffer.as_mut_ptr() as *mut c_void),
                buffer.len() as u32,
                Some(&mut returned),
                None,
            )
        };
        if ok.is_ok() {
            queried = true;
            break;
        }
        let more = ok
            .as_ref()
            .err()
            .is_some_and(|err| (err.code().0 & 0xFFFF) == ERROR_MORE_DATA.0 as i32)
            || unsafe { GetLastError() } == ERROR_MORE_DATA;
        if !more || buffer.len() >= 1024 * 1024 {
            break;
        }
        let required = read_u32(&buffer, 0).unwrap_or(0) as usize;
        let next = if required > buffer.len() {
            required
        } else {
            buffer.len() * 2
        };
        buffer.resize(next, 0);
    }
    if !queried {
        unsafe {
            let _ = CloseHandle(device);
        }
        return;
    }

    let count = read_u32(&buffer, 4).unwrap_or(0);
    let point = std::mem::size_of::<MountPoint>();
    // `MOUNTMGR_MOUNT_POINTS` already includes one `MOUNTMGR_MOUNT_POINT`.
    let directory = 8 + point + if count > 0 { (count as usize - 1) * point } else { 0 };
    if count > 4096 || directory > buffer.len() {
        unsafe {
            let _ = CloseHandle(device);
        }
        return;
    }
    for i in 0..count.min(4096) {
        let at = 8 + i as usize * std::mem::size_of::<MountPoint>();
        let Some(offset) = read_u32(&buffer, at) else {
            break;
        };
        let Some(byte_len) = read_u16(&buffer, at + 4) else {
            break;
        };
        if byte_len < 96 {
            continue;
        }
        let offset = offset as usize;
        let byte_len = byte_len as usize;
        if offset.saturating_add(byte_len) > buffer.len() || byte_len % 2 != 0 {
            continue;
        }
        let mut name = Vec::with_capacity(byte_len / 2);
        for chunk in buffer[offset..offset + byte_len].chunks_exact(2) {
            name.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        if !is_volume_name(&name) {
            continue;
        }
        let mut path = String::from_utf16_lossy(&name);
        if let Some(rest) = path.strip_prefix("\\??\\") {
            path = format!("\\\\?\\{rest}");
        }
        let wide_path = win::wide(&path);
        let volume = unsafe {
            CreateFileW(
                PCWSTR(wide_path.as_ptr()),
                (FILE_WRITE_DATA | SYNCHRONIZE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        };
        if let Ok(volume) = volume {
            unsafe {
                let _ = FlushFileBuffers(volume);
                let _ = CloseHandle(volume);
            }
        }
    }
    unsafe {
        let _ = CloseHandle(device);
    }
}

fn run(mask: u32) -> CleanResult {
    let mask = mask & MASK_ALL;
    if !win::is_elevated() {
        return CleanResult {
            status: CleanStatus::NotElevated,
            released_bytes: 0,
        };
    }
    enable_privilege(SE_PROF_SINGLE_PROCESS_NAME);
    enable_privilege(SE_INCREASE_QUOTA_NAME);
    let before = used_physical_bytes();

    if mask & WORKING_SET != 0 {
        let mut command = MEMORY_EMPTY_WORKING_SETS;
        set_info(
            SYSTEM_MEMORY_LIST_INFORMATION,
            &mut command as *mut u32 as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }
    if mask & SYSTEM_FILE_CACHE != 0 {
        let mut cache = FileCacheInformation {
            current_size: 0,
            peak_size: 0,
            page_fault_count: 0,
            minimum_working_set: usize::MAX,
            maximum_working_set: usize::MAX,
            current_size_including_transition: 0,
            peak_size_including_transition: 0,
            transition_repurpose_count: 0,
            flags: 0,
        };
        set_info(
            SYSTEM_FILE_CACHE_INFORMATION_EX,
            &mut cache as *mut FileCacheInformation as *mut c_void,
            std::mem::size_of::<FileCacheInformation>() as u32,
        );
    }
    if mask & MODIFIED_FILE_CACHE != 0 {
        flush_modified_file_cache();
    }
    if mask & MODIFIED_PAGE_LIST != 0 {
        let mut command = MEMORY_FLUSH_MODIFIED_LIST;
        set_info(
            SYSTEM_MEMORY_LIST_INFORMATION,
            &mut command as *mut u32 as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }
    if mask & STANDBY_LIST != 0 {
        let mut command = MEMORY_PURGE_STANDBY_LIST;
        set_info(
            SYSTEM_MEMORY_LIST_INFORMATION,
            &mut command as *mut u32 as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }
    if mask & STANDBY_PRIORITY0 != 0 {
        let mut command = MEMORY_PURGE_LOW_PRIORITY_STANDBY_LIST;
        set_info(
            SYSTEM_MEMORY_LIST_INFORMATION,
            &mut command as *mut u32 as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }
    if mask & REGISTRY_CACHE != 0 && windows_at_least(6, 3) {
        set_info(SYSTEM_REGISTRY_RECONCILIATION_INFORMATION, std::ptr::null_mut(), 0);
    }
    if mask & COMBINE_MEMORY_LISTS != 0 && windows_at_least(10, 0) {
        let mut combine = MemoryCombineInformationEx {
            handle: HANDLE::default(),
            pages_combined: 0,
            flags: 0,
        };
        set_info(
            SYSTEM_COMBINE_PHYSICAL_MEMORY_INFORMATION,
            &mut combine as *mut MemoryCombineInformationEx as *mut c_void,
            std::mem::size_of::<MemoryCombineInformationEx>() as u32,
        );
    }

    let after = used_physical_bytes();
    CleanResult {
        status: CleanStatus::Ok,
        released_bytes: before.saturating_sub(after),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_mask_falls_back_to_the_default_regions() {
        assert_eq!(normalize_mask(0), MASK_DEFAULT);
        assert_eq!(normalize_mask(MASK_DEFAULT), MASK_DEFAULT);
        assert_eq!(normalize_mask(0xFFFF), MASK_ALL);
        assert_eq!(normalize_mask(WORKING_SET), WORKING_SET);
    }

    #[test]
    fn byte_text_matches_the_plugin() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
    }

    #[test]
    fn volume_names_follow_the_mount_manager_shape() {
        let ok: Vec<u16> = "\\??\\Volume{01234567-89ab-cdef-0123-456789abcdef}"
            .encode_utf16()
            .collect();
        assert_eq!(ok.len(), 48);
        assert!(is_volume_name(&ok));
        let mut slash = ok.clone();
        slash.push('\\' as u16);
        assert!(is_volume_name(&slash));
        assert!(!is_volume_name(&ok[..47]));
    }
}
