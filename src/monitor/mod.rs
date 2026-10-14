pub mod hardware;
pub mod history;
pub mod net;
pub mod pdh;

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use crate::plugin::PluginHost;
use hardware::{HardwareFlags, HardwareMonitor, HardwareState};
use history::HistoryTraffic;
use net::Connection;

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub in_speed: u64,
    pub out_speed: u64,
    pub cpu_usage: i32,
    pub memory_usage: i32,
    pub used_memory_kb: u64,
    pub total_memory_kb: u64,
    pub gpu_usage: i32,
    pub hdd_usage: i32,
    pub cpu_freq: f32,
    pub cpu_temperature: f32,
    pub gpu_temperature: f32,
    pub hdd_temperature: f32,
    pub main_board_temperature: f32,
    pub today_up_traffic: u64,
    pub today_down_traffic: u64,
}

impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            in_speed: 0,
            out_speed: 0,
            cpu_usage: 0,
            memory_usage: 0,
            used_memory_kb: 0,
            total_memory_kb: 0,
            gpu_usage: -1,
            hdd_usage: -1,
            cpu_freq: -1.0,
            cpu_temperature: -1.0,
            gpu_temperature: -1.0,
            hdd_temperature: -1.0,
            main_board_temperature: -1.0,
            today_up_traffic: 0,
            today_down_traffic: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonitorParams {
    pub interval_ms: u32,
    pub cpu_by_time: bool,
    pub show_all_interface: bool,
    pub connections_hide: Vec<String>,
    pub hard_disk_name: String,
    pub cpu_core_name: String,
    pub hardware: HardwareFlags,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Selection {
    Auto,
    All,
    Index(usize),
    Named(String),
}

pub enum MonitorCmd {
    SetParams(MonitorParams),
    Refresh,
    Select(Selection),
    SaveHistory,
    Quit(Sender<()>),
}

#[derive(Clone, Debug)]
pub enum MonitorEvent {
    Snapshot(Snapshot),
    Connections {
        list: Vec<Connection>,
        selected: usize,
        selection: Selection,
    },
    HardwareLists {
        disks: Vec<String>,
        cpu_cores: Vec<String>,
    },
    HardwareState(HardwareState),
}

pub type EventSink = Box<dyn Fn(MonitorEvent) + Send>;

pub struct MonitorHandle {
    pub tx: Sender<MonitorCmd>,
    pub history: Arc<Mutex<HistoryTraffic>>,
}

impl MonitorHandle {
    pub fn send(&self, cmd: MonitorCmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn quit(&self) {
        let (tx, rx) = std::sync::mpsc::channel();
        if self.tx.send(MonitorCmd::Quit(tx)).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(3));
        }
    }
}

pub fn start(
    params: MonitorParams,
    selection: Selection,
    history: HistoryTraffic,
    plugins: Arc<PluginHost>,
    events: EventSink,
) -> MonitorHandle {
    let (tx, rx) = std::sync::mpsc::channel();
    let history = Arc::new(Mutex::new(history));
    let h = history.clone();
    std::thread::Builder::new()
        .name("monitor".into())
        .spawn(move || {
            let mut m = Monitor::new(params, selection, h, plugins, events);
            m.run(rx);
        })
        .expect("spawn monitor thread");
    MonitorHandle { tx, history }
}

struct Monitor {
    params: MonitorParams,
    selection: Selection,
    connections: Vec<Connection>,
    selected: usize,
    connection_changed: bool,
    last_in: u64,
    last_out: u64,
    last_time: Option<Instant>,
    zero_speed_cnt: u32,
    tick: u64,
    last_interface_count: Option<u32>,
    last_saved_kb: Option<u64>,
    today_up: u64,
    today_down: u64,
    history: Arc<Mutex<HistoryTraffic>>,
    cpu: pdh::CpuUsage,
    gpu: pdh::GpuUsage,
    disk: pdh::DiskUsage,
    freq: pdh::CpuFreq,
    hw: HardwareMonitor,
    plugins: Arc<PluginHost>,
    events: EventSink,
    last_lists: (Vec<String>, Vec<String>),
    last_hw_state: HardwareState,
}

impl Monitor {
    fn new(
        params: MonitorParams,
        selection: Selection,
        history: Arc<Mutex<HistoryTraffic>>,
        plugins: Arc<PluginHost>,
        events: EventSink,
    ) -> Self {
        let (today_up, today_down) = history.lock().unwrap().today_bytes();
        let mut m = Monitor {
            params,
            selection,
            connections: Vec::new(),
            selected: 0,
            connection_changed: true,
            last_in: 0,
            last_out: 0,
            last_time: None,
            zero_speed_cnt: 0,
            tick: 0,
            last_interface_count: None,
            last_saved_kb: None,
            today_up,
            today_down,
            history,
            cpu: pdh::CpuUsage::new(),
            gpu: pdh::GpuUsage::new(),
            disk: pdh::DiskUsage::new(),
            freq: pdh::CpuFreq::new(),
            hw: HardwareMonitor::new(),
            plugins,
            events,
            last_lists: (Vec::new(), Vec::new()),
            last_hw_state: HardwareState::Disabled,
        };
        m.hw.configure(m.params.hardware);
        m.last_hw_state = m.hw.state.clone();
        (m.events)(MonitorEvent::HardwareState(m.hw.state.clone()));
        m.init_connections();
        m
    }

    fn timer_count(&self, seconds: u64) -> u64 {
        (seconds * 1000 / self.params.interval_ms.max(1) as u64).max(1)
    }

    fn run(&mut self, rx: Receiver<MonitorCmd>) {
        let mut next = Instant::now();
        loop {
            let now = Instant::now();
            if now >= next {
                self.acquire();
                next = now + Duration::from_millis(self.params.interval_ms as u64);
            }
            let wait = next.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(MonitorCmd::SetParams(p)) => {
                    let reinit = p.show_all_interface != self.params.show_all_interface
                        || p.connections_hide != self.params.connections_hide;
                    let hw_changed = p.hardware != self.params.hardware;
                    let interval_changed = p.interval_ms != self.params.interval_ms;
                    self.params = p;
                    if reinit {
                        self.init_connections();
                    }
                    if hw_changed {
                        self.hw.configure(self.params.hardware);
                        self.last_hw_state = self.hw.state.clone();
                        (self.events)(MonitorEvent::HardwareState(self.hw.state.clone()));
                    }
                    if interval_changed {
                        next = Instant::now() + Duration::from_millis(self.params.interval_ms as u64);
                    }
                }
                Ok(MonitorCmd::Refresh) => self.init_connections(),
                Ok(MonitorCmd::Select(sel)) => {
                    self.selection = sel;
                    self.apply_selection();
                    self.publish_connections();
                }
                Ok(MonitorCmd::SaveHistory) => self.save_history(),
                Ok(MonitorCmd::Quit(done)) => {
                    self.save_history();
                    self.hw.stop();
                    let _ = done.send(());
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.save_history();
                    return;
                }
            }
        }
    }

    fn init_connections(&mut self) {
        self.connections = net::connections(self.params.show_all_interface, &self.params.connections_hide);
        if self.params.show_all_interface && self.selection == Selection::All {
            self.selection = Selection::Auto;
        }
        self.apply_selection();
        self.last_interface_count = Some(net::interface_count());
        self.publish_connections();
    }

    fn apply_selection(&mut self) {
        match self.selection.clone() {
            Selection::Auto => self.auto_select(),
            Selection::All => {}
            Selection::Index(i) => {
                self.selected = i.min(self.connections.len().saturating_sub(1));
            }
            Selection::Named(name) => {
                self.selected = self
                    .connections
                    .iter()
                    .position(|c| c.description == name)
                    .unwrap_or(0);
            }
        }
        self.connection_changed = true;
    }

    /// Picks the operational connection with the most traffic (`CTrafficMonitorDlg::AutoSelect`).
    fn auto_select(&mut self) {
        let rows = net::if_table();
        let mut best: Option<(usize, u64)> = None;
        for (i, c) in self.connections.iter().enumerate() {
            if let Some(r) = rows.iter().find(|r| r.luid == c.luid) {
                if r.oper_up {
                    let total = r.in_octets + r.out_octets;
                    if best.map(|b| total > b.1).unwrap_or(true) {
                        best = Some((i, total));
                    }
                }
            }
        }
        self.selected = best.map(|b| b.0).unwrap_or(0);
        self.connection_changed = true;
    }

    fn publish_connections(&self) {
        (self.events)(MonitorEvent::Connections {
            list: self.connections.clone(),
            selected: self.selected,
            selection: self.selection.clone(),
        });
    }

    fn save_history(&mut self) {
        let mut h = self.history.lock().unwrap();
        h.set_today(self.today_up, self.today_down);
        h.save();
    }

    fn acquire(&mut self) {
        let rows = net::if_table();
        let (in_bytes, out_bytes) = if self.selection == Selection::All {
            self.connections.iter().fold((0u64, 0u64), |acc, c| {
                match rows.iter().find(|r| r.luid == c.luid) {
                    Some(r) => (acc.0 + r.in_octets, acc.1 + r.out_octets),
                    None => acc,
                }
            })
        } else {
            self.connections
                .get(self.selected)
                .and_then(|c| rows.iter().find(|r| r.luid == c.luid))
                .map(|r| (r.in_octets, r.out_octets))
                .unwrap_or((0, 0))
        };

        let (cur_in, cur_out) = if (in_bytes == 0 && out_bytes == 0)
            || (self.last_in == 0 && self.last_out == 0)
            || self.connection_changed
            || self.last_in > in_bytes
            || self.last_out > out_bytes
        {
            (0, 0)
        } else {
            (in_bytes - self.last_in, out_bytes - self.last_out)
        };
        let now = Instant::now();
        let span_ms = match self.last_time {
            Some(t) => (now - t).as_millis().max(1) as u64,
            None => self.params.interval_ms as u64,
        };
        self.last_time = Some(now);
        let in_speed = cur_in * 1000 / span_ms;
        let out_speed = cur_out * 1000 / span_ms;
        self.connection_changed = false;
        self.last_in = in_bytes;
        self.last_out = out_bytes;

        if self.selection == Selection::Auto {
            if cur_in == 0 && cur_out == 0 {
                self.zero_speed_cnt += 1;
            } else {
                self.zero_speed_cnt = 0;
            }
            if self.zero_speed_cnt as u64 >= self.timer_count(30) {
                self.auto_select();
                self.publish_connections();
                self.zero_speed_cnt = 0;
            }
        }

        {
            let mut h = self.history.lock().unwrap();
            if h.check_date() {
                self.today_up = 0;
                self.today_down = 0;
                self.last_saved_kb = None;
                h.save();
            }
            self.today_up += cur_out;
            self.today_down += cur_in;
            h.set_today(self.today_up, self.today_down);
        }
        let n30 = self.timer_count(30);
        if self.tick % n30 == n30 - 1 {
            let kb = (self.today_up + self.today_down) / 1024;
            match self.last_saved_kb {
                None => self.last_saved_kb = Some(kb),
                Some(last) if kb.saturating_sub(last) >= 10240 => {
                    self.save_history();
                    self.last_saved_kb = Some(kb);
                }
                _ => {}
            }
        }

        let n3 = self.timer_count(3);
        if self.tick % n3 == n3 - 1 {
            let count = net::interface_count();
            let selected_missing = self.selection != Selection::All
                && self
                    .connections
                    .get(self.selected)
                    .map(|c| !rows.iter().any(|r| r.luid == c.luid))
                    .unwrap_or(!self.connections.is_empty());
            if self.last_interface_count.is_some_and(|c| c != count) || selected_missing {
                self.init_connections();
            }
        }

        let cpu_usage = self.cpu.usage(self.params.cpu_by_time);
        let mut cpu_freq = self.freq.freq().unwrap_or(-1.0);
        let mut gpu_usage = if self.params.hardware.gpu { -1 } else { self.gpu.usage().unwrap_or(-1) };
        let mut hdd_usage = -1;
        let mut hdd_by_pdh = false;
        if !self.params.hardware.hdd {
            if let Some((u, _)) = self.disk.usage(&self.params.hard_disk_name) {
                hdd_usage = u;
                hdd_by_pdh = true;
            }
        }

        let (memory_usage, used_kb, total_kb) = unsafe {
            let mut st = MEMORYSTATUSEX {
                dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                ..Default::default()
            };
            let _ = GlobalMemoryStatusEx(&mut st);
            (
                st.dwMemoryLoad as i32,
                (st.ullTotalPhys - st.ullAvailPhys) / 1024,
                st.ullTotalPhys / 1024,
            )
        };

        let mut snap = Snapshot {
            in_speed,
            out_speed,
            cpu_usage,
            memory_usage,
            used_memory_kb: used_kb,
            total_memory_kb: total_kb,
            gpu_usage,
            hdd_usage,
            cpu_freq,
            today_up_traffic: self.today_up,
            today_down_traffic: self.today_down,
            ..Default::default()
        };

        if self.params.hardware.any() {
            if let Some(info) = self.hw.poll() {
                if self.params.hardware.gpu {
                    snap.gpu_temperature = info.gpu_temp;
                    if gpu_usage < 0 && info.gpu_usage >= 0.0 {
                        gpu_usage = info.gpu_usage as i32;
                        snap.gpu_usage = gpu_usage;
                    }
                }
                if self.params.hardware.mbd {
                    snap.main_board_temperature = info.mbd_temp;
                }
                if self.params.hardware.cpu {
                    if cpu_freq < 0.0 {
                        cpu_freq = info.cpu_freq;
                        snap.cpu_freq = cpu_freq;
                    }
                    snap.cpu_temperature = if self.params.cpu_core_name == crate::i18n::tr("IDS_AVREAGE_TEMPERATURE")
                        || !info.cpu_temps.contains_key(&self.params.cpu_core_name)
                    {
                        info.cpu_temp
                    } else {
                        info.cpu_temps[&self.params.cpu_core_name]
                    };
                }
                if self.params.hardware.hdd {
                    snap.hdd_temperature = info
                        .hdd_temps
                        .get(&self.params.hard_disk_name)
                        .or_else(|| info.hdd_temps.values().next())
                        .copied()
                        .unwrap_or(-1.0);
                    if !hdd_by_pdh {
                        snap.hdd_usage = info
                            .hdd_usage
                            .get(&self.params.hard_disk_name)
                            .or_else(|| info.hdd_usage.values().next())
                            .map(|v| *v as i32)
                            .unwrap_or(-1);
                    }
                }
                let disks: Vec<String> = if self.params.hardware.hdd {
                    info.hdd_temps.keys().cloned().collect()
                } else {
                    self.disk.names().to_vec()
                };
                let mut cores: Vec<String> = info.cpu_temps.keys().cloned().collect();
                cores.insert(0, crate::i18n::tr("IDS_AVREAGE_TEMPERATURE"));
                self.send_lists(disks, cores);
            }
            let state = self.hw.state.clone();
            if state != self.last_hw_state {
                self.last_hw_state = state.clone();
                (self.events)(MonitorEvent::HardwareState(state));
            }
        } else {
            let disks = self.disk.names().to_vec();
            self.send_lists(disks, vec![crate::i18n::tr("IDS_AVREAGE_TEMPERATURE")]);
        }

        self.plugins.data_required(&snap);

        self.tick += 1;
        (self.events)(MonitorEvent::Snapshot(snap));
    }

    fn send_lists(&mut self, disks: Vec<String>, cores: Vec<String>) {
        if self.last_lists.0 != disks || self.last_lists.1 != cores {
            self.last_lists = (disks.clone(), cores.clone());
            (self.events)(MonitorEvent::HardwareLists { disks, cpu_cores: cores });
        }
    }
}
