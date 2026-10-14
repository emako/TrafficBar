use std::collections::HashMap;

use windows::Win32::System::Performance::{
    PdhAddCounterW, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
    PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};
use windows::core::PCWSTR;

use crate::win::{from_pcwstr, wide};

/// Port of `CPdhQuery`.
pub struct PdhQuery {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
    ok: bool,
}

unsafe impl Send for PdhQuery {}

impl PdhQuery {
    pub fn new(path: &str) -> Self {
        let mut q = PdhQuery {
            query: PDH_HQUERY::default(),
            counter: PDH_HCOUNTER::default(),
            ok: false,
        };
        unsafe {
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut q.query) != 0 {
                return q;
            }
            let p = wide(path);
            let mut status = PdhAddCounterW(q.query, PCWSTR(p.as_ptr()), 0, &mut q.counter);
            if status != 0 {
                status = PdhAddEnglishCounterW(q.query, PCWSTR(p.as_ptr()), 0, &mut q.counter);
            }
            if status != 0 {
                PdhCloseQuery(q.query);
                q.query = PDH_HQUERY::default();
                return q;
            }
            PdhCollectQueryData(q.query);
            q.ok = true;
        }
        q
    }

    pub fn value(&mut self) -> Option<f64> {
        if !self.ok {
            return None;
        }
        unsafe {
            PdhCollectQueryData(self.query);
            let mut v = PDH_FMT_COUNTERVALUE::default();
            let status = PdhGetFormattedCounterValue(self.counter, PDH_FMT_DOUBLE, None, &mut v);
            if status != 0 {
                return None;
            }
            Some(v.Anonymous.doubleValue)
        }
    }

    pub fn values(&mut self) -> Option<Vec<(String, f64)>> {
        if !self.ok {
            return None;
        }
        unsafe {
            PdhCollectQueryData(self.query);
            let mut size = 0u32;
            let mut count = 0u32;
            let status = PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, None);
            if status != PDH_MORE_DATA {
                return None;
            }
            let mut buf = vec![0u8; size as usize + 16];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            let status = PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items));
            if status != 0 {
                return None;
            }
            let mut out = Vec::with_capacity(count as usize);
            for i in 0..count as usize {
                let item = &*items.add(i);
                out.push((from_pcwstr(item.szName.0), item.FmtValue.Anonymous.doubleValue));
            }
            Some(out)
        }
    }
}

impl Drop for PdhQuery {
    fn drop(&mut self) {
        if self.ok {
            unsafe {
                PdhCloseQuery(self.query);
            }
        }
    }
}

pub struct GpuUsage(PdhQuery);

impl GpuUsage {
    pub fn new() -> Self {
        GpuUsage(PdhQuery::new(r"\GPU Engine(*)\Utilization Percentage"))
    }

    /// Largest per-engine-type sum, like Task Manager (`CPdhGPUUsage::GetGpuUsage`).
    pub fn usage(&mut self) -> Option<i32> {
        let values = self.0.values()?;
        if values.is_empty() {
            return None;
        }
        let mut by_type: HashMap<String, f64> = HashMap::new();
        for (name, v) in values {
            let t = match name.rfind('_') {
                Some(i) => name[i + 1..].to_string(),
                None => name,
            };
            *by_type.entry(t).or_default() += v;
        }
        let max = by_type.values().cloned().fold(0.0, f64::max);
        Some(((max + 0.5) as i32).clamp(0, 100))
    }
}

pub struct DiskUsage {
    q: PdhQuery,
    names: Vec<String>,
}

impl DiskUsage {
    pub fn new() -> Self {
        let mut q = PdhQuery::new(r"\PhysicalDisk(*)\% Idle Time");
        let names = q.values().unwrap_or_default().into_iter().map(|(n, _)| n).collect();
        DiskUsage { q, names }
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Returns (usage, resolved disk name). Falls back to `_Total`, then the first disk.
    pub fn usage(&mut self, name: &str) -> Option<(i32, String)> {
        let values = self.q.values()?;
        if values.is_empty() {
            return None;
        }
        self.names = values.iter().map(|(n, _)| n.clone()).collect();
        let pick = values
            .iter()
            .find(|(n, _)| n == name)
            .or_else(|| values.iter().find(|(n, _)| n == "_Total"))
            .or_else(|| values.first())?;
        let idle = pick.1.clamp(0.0, 100.0);
        Some(((100.0 - idle + 0.5) as i32, pick.0.clone()))
    }
}

pub struct CpuFreq {
    q: PdhQuery,
    max_ghz: f32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ProcessorPowerInformation {
    number: u32,
    max_mhz: u32,
    current_mhz: u32,
    mhz_limit: u32,
    max_idle_state: u32,
    current_idle_state: u32,
}

impl CpuFreq {
    pub fn new() -> Self {
        use windows::Win32::System::Power::{CallNtPowerInformation, ProcessorInformation};
        use windows::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
        let mut max_ghz = 0f32;
        unsafe {
            let mut si = SYSTEM_INFO::default();
            GetSystemInfo(&mut si);
            let n = si.dwNumberOfProcessors.max(1) as usize;
            let mut info = vec![ProcessorPowerInformation::default(); n];
            let status = CallNtPowerInformation(
                ProcessorInformation,
                None,
                0,
                Some(info.as_mut_ptr() as *mut _),
                (std::mem::size_of::<ProcessorPowerInformation>() * n) as u32,
            );
            if status.is_ok() {
                for p in &info {
                    max_ghz = max_ghz.max(p.max_mhz as f32 / 1000.0);
                }
            }
        }
        CpuFreq {
            q: PdhQuery::new(r"\Processor Information(_Total)\% Processor Performance"),
            max_ghz,
        }
    }

    pub fn freq(&mut self) -> Option<f32> {
        self.q.value().map(|v| (v / 100.0) as f32 * self.max_ghz)
    }
}

pub struct CpuUsage {
    pdh: PdhQuery,
    prev: Option<(u64, u64, u64)>,
}

impl CpuUsage {
    pub fn new() -> Self {
        let path = if crate::win::windows_version().major >= 10 {
            r"\Processor Information(_Total)\% Processor Utility"
        } else {
            r"\Processor Information(_Total)\% Processor Time"
        };
        CpuUsage {
            pdh: PdhQuery::new(path),
            prev: None,
        }
    }

    pub fn usage(&mut self, use_cpu_time: bool) -> i32 {
        if !use_cpu_time {
            if let Some(v) = self.pdh.value() {
                return (v as i32).min(100);
            }
        }
        self.by_system_times()
    }

    fn by_system_times(&mut self) -> i32 {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Threading::GetSystemTimes;
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe {
            let _ = GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user));
        }
        let f = |t: FILETIME| ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64;
        let cur = (f(idle), f(kernel), f(user));
        let usage = match self.prev {
            Some(p) => {
                let di = cur.0.wrapping_sub(p.0) as i64;
                let dk = cur.1.wrapping_sub(p.1) as i64;
                let du = cur.2.wrapping_sub(p.2) as i64;
                if dk + du == 0 { 0 } else { ((dk + du - di) * 100 / (dk + du)).abs() as i32 }
            }
            None => 0,
        };
        self.prev = Some(cur);
        usage
    }
}
