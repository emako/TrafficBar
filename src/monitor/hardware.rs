use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use serde::Deserialize;

const RESULT_CAP: usize = 256 * 1024;

static HARDWARE_DLL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/TrafficBar.Hardware.dll"));

unsafe extern "C" {
    fn hardware_clr_call(
        assembly: *const u8,
        assembly_len: usize,
        arg: *const u16,
        ret: *mut u32,
    ) -> i32;
}

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
    Running,
    Failed(String),
}

/// Hosts the embedded LibreHardwareMonitor assembly inside this process.
/// `clr_host.cpp` loads the bytes with `_AppDomain::Load_3`; nothing is written to disk.
pub struct HardwareMonitor {
    mailbox: Arc<Mailbox>,
    latest: Arc<Mutex<Option<HardwareInfo>>>,
    shared_state: Arc<Mutex<HardwareState>>,
    worker: Option<JoinHandle<()>>,
    flags: HardwareFlags,
    pub state: HardwareState,
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
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
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
            && matches!(
                self.state,
                HardwareState::Running | HardwareState::Failed(_)
            )
        {
            return;
        }
        self.flags = flags;
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
        match std::thread::Builder::new()
            .name("hardware".into())
            .spawn(move || worker(mailbox, latest, shared_state))
        {
            Ok(handle) => {
                self.worker = Some(handle);
                true
            }
            Err(err) => {
                self.assign(
                    HardwareState::Failed(format!("failed to start hardware thread: {err}")),
                    false,
                );
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
        mail.job = Some(Job {
            generation: mail.generation,
            flags,
        });
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

fn worker(
    mailbox: Arc<Mailbox>,
    latest: Arc<Mutex<Option<HardwareInfo>>>,
    shared_state: Arc<Mutex<HardwareState>>,
) {
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
                    host = Some(ClrHost);
                }
                let Some(host_ref) = host.as_ref() else {
                    continue;
                };
                match open_monitor(host_ref, flags) {
                    Ok(()) => {
                        let info = match sample(host_ref) {
                            Ok(info) => info,
                            Err(err) => HardwareInfo {
                                error: Some(err),
                                ..HardwareInfo::default()
                            },
                        };
                        let state = match info.error.clone() {
                            Some(err) => HardwareState::Failed(err),
                            None => HardwareState::Running,
                        };
                        if publish(
                            &mailbox,
                            &latest,
                            &shared_state,
                            job.generation,
                            state,
                            Some(info),
                        ) {
                            opened = true;
                            session = job.generation;
                        } else {
                            let _ = close_monitor(host_ref);
                        }
                    }
                    Err(err) => {
                        publish(
                            &mailbox,
                            &latest,
                            &shared_state,
                            job.generation,
                            HardwareState::Failed(err),
                            None,
                        );
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
                        let _ =
                            publish(&mailbox, &latest, &shared_state, session, state, Some(info));
                    }
                    Err(err) => {
                        let _ = publish(
                            &mailbox,
                            &latest,
                            &shared_state,
                            session,
                            HardwareState::Failed(err),
                            None,
                        );
                    }
                }
            }
        }
    }
}

fn open_monitor(host: &ClrHost, flags: HardwareFlags) -> Result<(), String> {
    let payload = format!(
        "{}\t{}\t{}\t{}",
        u8::from(flags.cpu),
        u8::from(flags.gpu),
        u8::from(flags.hdd),
        u8::from(flags.mbd)
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
    let text = host.call(op, payload)?;
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
            inner: Mutex::new(Mail {
                generation: 0,
                job: None,
                update: false,
                shutdown: false,
            }),
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

/// CLR v4 host. Startup and `Load_3` live in `clr_host.cpp`. The runtime is kept for
/// the life of the process: stopping it can hang once managed code has run.
struct ClrHost;

impl ClrHost {
    fn call(&self, op: &str, payload: &str) -> Result<String, String> {
        let mut buf = vec![0u8; RESULT_CAP];
        let arg = format!(
            "{}\n{}\n{op}\n{payload}",
            buf.as_mut_ptr() as usize,
            buf.len()
        );
        let arg_w = crate::win::wide(&arg);
        let mut ret = 0u32;
        let hr = unsafe {
            hardware_clr_call(
                HARDWARE_DLL.as_ptr(),
                HARDWARE_DLL.len(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_init_is_reported_in_process() {
        let host = ClrHost;
        let text = host.call("init", "cpu").expect("dispatch");
        let info: HardwareInfo = serde_json::from_str(&text).expect("json");
        assert!(
            info.error.as_deref().is_some_and(|err| !err.is_empty()),
            "{text}"
        );
        let closed = host.call("close", "").expect("close");
        assert_eq!(closed, "{}");
    }
}
