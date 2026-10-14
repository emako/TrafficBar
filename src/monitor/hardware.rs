use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::Deserialize;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub const HELPER_EXE: &str = "TrafficMonitorBar.Hardware.exe";
pub const LHM_DLL: &str = "LibreHardwareMonitorLib.dll";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct HardwareInfo {
    pub cpu_temp: f32,
    pub cpu_freq: f32,
    pub gpu_temp: f32,
    pub gpu_usage: f32,
    pub mbd_temp: f32,
    pub cpu_temps: BTreeMap<String, f32>,
    pub hdd_temps: BTreeMap<String, f32>,
    pub hdd_usage: BTreeMap<String, f32>,
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
    HelperMissing,
    DllMissing,
    Running,
    Failed(String),
}

/// Talks to `TrafficMonitorBar.Hardware.exe`, which hosts LibreHardwareMonitor out of process.
pub struct HardwareMonitor {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    flags: HardwareFlags,
    latest: Arc<Mutex<Option<HardwareInfo>>>,
    pub state: HardwareState,
}

pub fn helper_path() -> PathBuf {
    crate::win::exe_dir().join(HELPER_EXE)
}

pub fn lhm_path() -> PathBuf {
    crate::win::exe_dir().join(LHM_DLL)
}

impl HardwareMonitor {
    pub fn new() -> Self {
        HardwareMonitor {
            child: None,
            stdin: None,
            flags: HardwareFlags::default(),
            latest: Arc::new(Mutex::new(None)),
            state: HardwareState::Disabled,
        }
    }

    pub fn configure(&mut self, flags: HardwareFlags) {
        if !flags.any() {
            self.stop();
            self.flags = flags;
            self.state = HardwareState::Disabled;
            return;
        }
        if !crate::win::is_elevated() {
            self.stop();
            self.flags = flags;
            self.state = HardwareState::NotElevated;
            return;
        }
        if flags == self.flags && self.child.is_some() {
            return;
        }
        self.stop();
        self.flags = flags;
        let helper = helper_path();
        if !helper.exists() {
            self.state = HardwareState::HelperMissing;
            return;
        }
        let dll = lhm_path();
        if !dll.exists() {
            self.state = HardwareState::DllMissing;
            return;
        }
        let mut cmd = Command::new(helper);
        if flags.cpu {
            cmd.arg("--cpu");
        }
        if flags.gpu {
            cmd.arg("--gpu");
        }
        if flags.hdd {
            cmd.arg("--hdd");
        }
        if flags.mbd {
            cmd.arg("--mbd");
        }
        cmd.arg("--lhm").arg(dll);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        cmd.creation_flags(CREATE_NO_WINDOW);
        match cmd.spawn() {
            Ok(mut child) => {
                let stdout = child.stdout.take();
                self.stdin = child.stdin.take();
                let latest = self.latest.clone();
                if let Some(out) = stdout {
                    std::thread::Builder::new()
                        .name("hardware-reader".into())
                        .spawn(move || {
                            let reader = BufReader::new(out);
                            for line in reader.lines() {
                                let Ok(line) = line else { break };
                                if let Ok(info) = serde_json::from_str::<HardwareInfo>(&line) {
                                    *latest.lock().unwrap() = Some(info);
                                }
                            }
                        })
                        .ok();
                }
                self.child = Some(child);
                self.state = HardwareState::Running;
            }
            Err(e) => {
                self.state = HardwareState::Failed(e.to_string());
            }
        }
    }

    /// Requests a new sample and returns the most recent one received.
    pub fn poll(&mut self) -> Option<HardwareInfo> {
        if let Some(stdin) = self.stdin.as_mut() {
            if stdin.write_all(b"update\n").and_then(|_| stdin.flush()).is_err() {
                self.state = HardwareState::Failed("hardware helper exited".into());
                self.stdin = None;
            }
        }
        let info = self.latest.lock().unwrap().clone();
        if let Some(i) = &info {
            if let Some(err) = &i.error {
                self.state = HardwareState::Failed(err.clone());
            }
        }
        info
    }

    pub fn stop(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.write_all(b"exit\n");
        }
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                let _ = child.kill();
                let _ = child.wait();
            });
        }
        *self.latest.lock().unwrap() = None;
    }
}

impl Drop for HardwareMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}
