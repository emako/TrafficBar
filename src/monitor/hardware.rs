use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use serde::Deserialize;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::{GUID, s, w};

const BRIDGE_DLL: &str = "TrafficMonitorBar.Hardware.dll";
pub const LHM_DLL: &str = "LibreHardwareMonitorLib.dll";
const RESULT_CAP: usize = 256 * 1024;

const CLSID_CLR_META_HOST: GUID = GUID::from_u128(0x9280188d_0e8e_4867_b30c_7fa83884e8de);
const IID_ICLR_META_HOST: GUID = GUID::from_u128(0xD332DB9E_B9B3_4125_8207_A14884F53216);
const IID_ICLR_RUNTIME_INFO: GUID = GUID::from_u128(0xBD39D1D2_BA2F_486a_89B0_B4B0CB466891);
const CLSID_CLR_RUNTIME_HOST: GUID = GUID::from_u128(0x90F1A06E_7712_4762_86B5_7A5EBA6BDB02);
const IID_ICLR_RUNTIME_HOST: GUID = GUID::from_u128(0x90F1A06C_7712_4762_86B5_7A5EBA6BDB02);

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct HardwareInfo {
    pub cpu_temp: f32,
    pub cpu_freq: f32,
    pub gpu_temp: f32,
    pub gpu_usage: f32,
    pub mbd_temp: f32,
    pub cpu_temps: std::collections::BTreeMap<String, f32>,
    pub hdd_temps: std::collections::BTreeMap<String, f32>,
    pub hdd_usage: std::collections::BTreeMap<String, f32>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HardwareFlags {
    pub cpu: bool,
    pub gpu: bool,
    pub hdd: bool,
    pub mbd: bool,
}

impl HardwareFlags {
    pub fn any(&self) -> bool {
        self.cpu || self.gpu || self.hdd || self.mbd
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HardwareState {
    Disabled,
    NotElevated,
    LibraryMissing,
    DllMissing,
    Running,
    Failed(String),
}

/// Hosts LibreHardwareMonitor inside this process, the same way the original
/// TrafficMonitor loads OpenHardwareMonitorApi.dll.
pub struct HardwareMonitor {
    mailbox: Arc<Mailbox>,
    latest: Arc<Mutex<Option<HardwareInfo>>>,
    shared_state: Arc<Mutex<HardwareState>>,
    worker: Option<JoinHandle<()>>,
    flags: HardwareFlags,
    pub state: HardwareState,
}

pub fn lhm_path() -> PathBuf {
    crate::win::exe_dir().join(LHM_DLL)
}

fn bridge_path() -> PathBuf {
    crate::win::exe_dir().join(BRIDGE_DLL)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

impl HardwareMonitor {
    pub fn new() -> Self {
        let state = HardwareState::Disabled;
        HardwareMonitor {
            mailbox: Arc::new(Mailbox::new()),
            latest: Arc::new(Mutex::new(None)),
            shared_state: Arc::new(Mutex::new(state.clone())),
            worker: None,
            flags: HardwareFlags::default(),
            state,
        }
    }

    pub fn configure(&mut self, flags: HardwareFlags) {
        if self.worker.as_ref().is_some_and(|worker| worker.is_finished()) {
            self.worker = None;
        }
        if !flags.any() {
            self.flags = flags;
            self.submit(None, HardwareState::Disabled);
            return;
        }
        if !crate::win::is_elevated() {
            self.flags = flags;
            self.submit(None, HardwareState::NotElevated);
            return;
        }
        if flags == self.flags
            && self.worker.is_some()
            && matches!(self.state, HardwareState::Running | HardwareState::Failed(_))
        {
            return;
        }
        self.flags = flags;
        if !lhm_path().is_file() {
            self.submit(None, HardwareState::DllMissing);
            return;
        }
        if !bridge_path().is_file() {
            self.submit(None, HardwareState::LibraryMissing);
            return;
        }
        if !self.ensure_worker() {
            return;
        }
        self.submit(Some(flags), HardwareState::Running);
    }

    /// Returns the most recent sample. Sampling itself runs on the hardware thread.
    pub fn poll(&mut self) -> Option<HardwareInfo> {
        if self.flags.any() && self.worker.is_some() {
            self.mailbox.request_update();
        }
        self.state = lock(&self.shared_state).clone();
        if !self.flags.any() {
            return None;
        }
        lock(&self.latest).clone()
    }

    pub fn stop(&mut self) {
        self.mailbox.shutdown();
    }

    fn ensure_worker(&mut self) -> bool {
        if self.worker.is_some() {
            return true;
        }
        let mailbox = Arc::clone(&self.mailbox);
        let latest = Arc::clone(&self.latest);
        let shared_state = Arc::clone(&self.shared_state);
        match std::thread::Builder::new().name("hardware".into()).spawn(move || worker(mailbox, latest, shared_state)) {
            Ok(handle) => {
                self.worker = Some(handle);
                true
            }
            Err(err) => {
                self.assign(HardwareState::Failed(format!("failed to start hardware thread: {err}")), false);
                false
            }
        }
    }

    /// Posts a new open/close request. Passing `None` closes LibreHardwareMonitor.
    /// An in-flight open will not overwrite `state`, because this bumps the generation.
    fn submit(&mut self, flags: Option<HardwareFlags>, state: HardwareState) {
        if flags.is_none() && self.worker.is_none() {
            self.assign(state, true);
            return;
        }
        let mut mail = lock(&self.mailbox.inner);
        mail.generation = mail.generation.wrapping_add(1);
        mail.job = Some(Job { generation: mail.generation, flags });
        *lock(&self.latest) = None;
        self.state = state.clone();
        *lock(&self.shared_state) = state;
        self.mailbox.cvar.notify_one();
    }

    fn assign(&mut self, state: HardwareState, clear_latest: bool) {
        let _mail = lock(&self.mailbox.inner);
        if clear_latest {
            *lock(&self.latest) = None;
        }
        self.state = state.clone();
        *lock(&self.shared_state) = state;
    }
}

impl Drop for HardwareMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

fn worker(mailbox: Arc<Mailbox>, latest: Arc<Mutex<Option<HardwareInfo>>>, shared_state: Arc<Mutex<HardwareState>>) {
    let mut host: Option<ClrHost> = None;
    let mut opened = false;
    let mut session = 0u64;
    loop {
        match mailbox.wait() {
            Action::Shutdown => {
                if opened {
                    if let Some(host) = host.as_ref() {
                        let _ = close_monitor(host);
                    }
                }
                break;
            }
            Action::Configure(job) => {
                if opened {
                    if let Some(host) = host.as_ref() {
                        let _ = close_monitor(host);
                    }
                    opened = false;
                }
                let Some(flags) = job.flags else { continue };
                if host.is_none() {
                    match ClrHost::start() {
                        Ok(started) => host = Some(started),
                        Err(err) => {
                            publish(&mailbox, &latest, &shared_state, job.generation, HardwareState::Failed(err), None);
                            continue;
                        }
                    }
                }
                let Some(host_ref) = host.as_ref() else { continue };
                match open_monitor(host_ref, flags) {
                    Ok(()) => {
                        let info = match sample(host_ref) {
                            Ok(info) => info,
                            Err(err) => HardwareInfo { error: Some(err), ..HardwareInfo::default() },
                        };
                        let state = match info.error.clone() {
                            Some(err) => HardwareState::Failed(err),
                            None => HardwareState::Running,
                        };
                        if publish(&mailbox, &latest, &shared_state, job.generation, state, Some(info)) {
                            opened = true;
                            session = job.generation;
                        } else {
                            let _ = close_monitor(host_ref);
                        }
                    }
                    Err(err) => {
                        publish(&mailbox, &latest, &shared_state, job.generation, HardwareState::Failed(err), None);
                    }
                }
            }
            Action::Update => {
                if !opened {
                    continue;
                }
                let Some(host) = host.as_ref() else { continue };
                match sample(host) {
                    Ok(info) => {
                        let state = match info.error.clone() {
                            Some(err) => HardwareState::Failed(err),
                            None => HardwareState::Running,
                        };
                        let _ = publish(&mailbox, &latest, &shared_state, session, state, Some(info));
                    }
                    Err(err) => {
                        let _ = publish(&mailbox, &latest, &shared_state, session, HardwareState::Failed(err), None);
                    }
                }
            }
        }
    }
}

fn open_monitor(host: &ClrHost, flags: HardwareFlags) -> Result<(), String> {
    let payload = format!(
        "{}\t{}\t{}\t{}\t{}",
        u8::from(flags.cpu),
        u8::from(flags.gpu),
        u8::from(flags.hdd),
        u8::from(flags.mbd),
        lhm_path().display()
    );
    let info = call_info(host, "init", &payload)?;
    match info.error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn sample(host: &ClrHost) -> Result<HardwareInfo, String> {
    call_info(host, "update", "")
}

fn close_monitor(host: &ClrHost) -> Result<(), String> {
    let _ = call_info(host, "close", "")?;
    Ok(())
}

fn call_info(host: &ClrHost, op: &str, payload: &str) -> Result<HardwareInfo, String> {
    let text = host.call(&bridge_path(), op, payload)?;
    serde_json::from_str(&text).map_err(|err| format!("bad hardware response: {err}"))
}

fn publish(
    mailbox: &Mailbox,
    latest: &Mutex<Option<HardwareInfo>>,
    shared_state: &Mutex<HardwareState>,
    generation: u64,
    state: HardwareState,
    info: Option<HardwareInfo>,
) -> bool {
    let mail = lock(&mailbox.inner);
    if mail.generation != generation {
        return false;
    }
    *lock(latest) = info;
    *lock(shared_state) = state;
    true
}

struct Job {
    generation: u64,
    flags: Option<HardwareFlags>,
}

enum Action {
    Configure(Job),
    Update,
    Shutdown,
}

struct Mail {
    generation: u64,
    job: Option<Job>,
    update: bool,
    shutdown: bool,
}

struct Mailbox {
    inner: Mutex<Mail>,
    cvar: Condvar,
}

impl Mailbox {
    fn new() -> Self {
        Mailbox {
            inner: Mutex::new(Mail { generation: 0, job: None, update: false, shutdown: false }),
            cvar: Condvar::new(),
        }
    }

    fn request_update(&self) {
        let mut mail = lock(&self.inner);
        if mail.shutdown {
            return;
        }
        mail.update = true;
        self.cvar.notify_one();
    }

    fn shutdown(&self) {
        let mut mail = lock(&self.inner);
        mail.shutdown = true;
        mail.generation = mail.generation.wrapping_add(1);
        self.cvar.notify_one();
    }

    fn wait(&self) -> Action {
        let mut mail = lock(&self.inner);
        loop {
            if mail.shutdown {
                return Action::Shutdown;
            }
            if let Some(job) = mail.job.take() {
                mail.update = false;
                return Action::Configure(job);
            }
            if mail.update {
                mail.update = false;
                return Action::Update;
            }
            mail = self.cvar.wait(mail).unwrap_or_else(|err| err.into_inner());
        }
    }
}

/// CLR v4 host. The runtime is intentionally kept for the life of the process:
/// `ICLRRuntimeHost::Stop` can hang once managed code has run.
struct ClrHost {
    runtime: *mut c_void,
}

type ClrCreateInstance = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> i32;

#[repr(C)]
struct IUnknownVtbl {
    _query: *const c_void,
    _add: *const c_void,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct MetaHostVtbl {
    _query: *const c_void,
    _add: *const c_void,
    _release: *const c_void,
    // ICLRMetaHost::GetRuntime
    get_runtime: unsafe extern "system" fn(*mut c_void, *const u16, *const GUID, *mut *mut c_void) -> i32,
}

#[repr(C)]
struct RuntimeInfoVtbl {
    _query: *const c_void,
    _add: *const c_void,
    _release: *const c_void,
    _get_version_string: *const c_void,
    _get_runtime_directory: *const c_void,
    _is_loaded: *const c_void,
    _load_error_string: *const c_void,
    _load_library: *const c_void,
    _get_proc_address: *const c_void,
    // ICLRRuntimeInfo::GetInterface
    get_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *const GUID, *mut *mut c_void) -> i32,
}

#[repr(C)]
struct RuntimeHostVtbl {
    _query: *const c_void,
    _add: *const c_void,
    _release: *const c_void,
    start: unsafe extern "system" fn(*mut c_void) -> i32,
    _stop: *const c_void,
    _set_host_control: *const c_void,
    _get_clr_control: *const c_void,
    _unload_app_domain: *const c_void,
    _execute_in_app_domain: *const c_void,
    _get_current_app_domain_id: *const c_void,
    _execute_application: *const c_void,
    // ICLRRuntimeHost::ExecuteInDefaultAppDomain
    execute: unsafe extern "system" fn(*mut c_void, *const u16, *const u16, *const u16, *const u16, *mut u32) -> i32,
}

impl ClrHost {
    fn start() -> Result<Self, String> {
        unsafe {
            let module = LoadLibraryW(w!("mscoree.dll")).map_err(|err| format!("failed to load mscoree.dll: {err}"))?;
            let Some(proc) = GetProcAddress(module, s!("CLRCreateInstance")) else {
                return Err("CLRCreateInstance was not found in mscoree.dll".into());
            };
            let create: ClrCreateInstance = std::mem::transmute(proc);
            let mut meta = std::ptr::null_mut();
            let hr = create(&CLSID_CLR_META_HOST, &IID_ICLR_META_HOST, &mut meta);
            if hr < 0 || meta.is_null() {
                return Err(format!("CLRCreateInstance failed: 0x{:08X}", hr as u32));
            }
            let version = crate::win::wide("v4.0.30319");
            let mut info = std::ptr::null_mut();
            let hr = ((*vtbl::<MetaHostVtbl>(meta)).get_runtime)(meta, version.as_ptr(), &IID_ICLR_RUNTIME_INFO, &mut info);
            if hr < 0 || info.is_null() {
                release(meta);
                return Err(format!(".NET Framework 4 runtime is unavailable: 0x{:08X}", hr as u32));
            }
            let mut runtime = std::ptr::null_mut();
            let hr = ((*vtbl::<RuntimeInfoVtbl>(info)).get_interface)(
                info,
                &CLSID_CLR_RUNTIME_HOST,
                &IID_ICLR_RUNTIME_HOST,
                &mut runtime,
            );
            release(info);
            release(meta);
            if hr < 0 || runtime.is_null() {
                return Err(format!("ICLRRuntimeHost is unavailable: 0x{:08X}", hr as u32));
            }
            let hr = ((*vtbl::<RuntimeHostVtbl>(runtime)).start)(runtime);
            if hr < 0 {
                release(runtime);
                return Err(format!("failed to start the CLR: 0x{:08X}", hr as u32));
            }
            Ok(ClrHost { runtime })
        }
    }

    fn call(&self, bridge: &Path, op: &str, payload: &str) -> Result<String, String> {
        let mut buf = vec![0u8; RESULT_CAP];
        let arg = format!("{}\n{}\n{op}\n{payload}", buf.as_mut_ptr() as usize, buf.len());
        let arg_w = crate::win::wide(&arg);
        let assembly = wide_path(bridge);
        let mut ret = 0u32;
        let hr = unsafe {
            ((*vtbl::<RuntimeHostVtbl>(self.runtime)).execute)(
                self.runtime,
                assembly.as_ptr(),
                w!("TrafficMonitorBar.Hardware.Bridge").as_ptr(),
                w!("Dispatch").as_ptr(),
                arg_w.as_ptr(),
                &mut ret,
            )
        };
        if hr < 0 {
            return Err(format!("CLR call failed: 0x{:08X}", hr as u32));
        }
        match ret {
            0 => {
                let len = buf.iter().position(|byte| *byte == 0).unwrap_or(0);
                String::from_utf8(buf[..len].to_vec()).map_err(|err| err.to_string())
            }
            2 => Err("hardware response exceeded the buffer".into()),
            code => Err(format!("hardware bridge returned {code}")),
        }
    }
}

unsafe fn vtbl<T>(obj: *mut c_void) -> *const T {
    unsafe { *(obj as *const *const T) }
}

unsafe fn release(obj: *mut c_void) {
    if !obj.is_null() {
        unsafe { ((*vtbl::<IUnknownVtbl>(obj)).release)(obj) };
    }
}

fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_library_is_reported_in_process() {
        let bridge = Path::new(env!("OUT_DIR")).ancestors().nth(3).expect("profile dir").join(BRIDGE_DLL);
        assert!(bridge.is_file(), "missing {}", bridge.display());
        let host = ClrHost::start().expect("clr");
        let text = host
            .call(&bridge, "init", "1\t0\t0\t0\tC:\\no-such\\LibreHardwareMonitorLib.dll")
            .expect("dispatch");
        let info: HardwareInfo = serde_json::from_str(&text).expect("json");
        assert!(info.error.as_deref().is_some_and(|err| !err.is_empty()), "{text}");
        let closed = host.call(&bridge, "close", "").expect("close");
        assert_eq!(closed, "{}");
    }
}
