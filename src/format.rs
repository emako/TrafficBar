//! Text formatting identical to the helpers in the original `Common.cpp` / `DisplayItem.cpp`.

use crate::config::{MemoryDisplay, SpeedUnit, TaskbarSettings};
use crate::display_item::DisplayItem;
use crate::monitor::Snapshot;

/// `CCommon::DataSizeToString(size, cfg)` — network speed text (without the `/s` suffix).
pub fn speed_to_string(size: u64, cfg: &TaskbarSettings) -> String {
    let mut size = size;
    if !cfg.unit_byte {
        size = size.saturating_mul(8);
    }
    let kb = size as f32 / 1024.0;
    let mb = kb / 1024.0;
    let gb = mb / 1024.0;
    let (value, unit): (String, &str) = match cfg.speed_unit {
        SpeedUnit::Auto => {
            if cfg.speed_short_mode {
                if size < 1024 * 10 {
                    (format!("{kb:.1}"), "K")
                } else if size < 1024 * 1000 {
                    (format!("{kb:.0}"), "K")
                } else if size < 1024 * 1024 * 1000 {
                    (format!("{mb:.1}"), "M")
                } else {
                    (format!("{gb:.2}"), "G")
                }
            } else if size < 1024 * 10 {
                (format!("{kb:.2}"), "KB")
            } else if size < 1024 * 1000 {
                (format!("{kb:.1}"), "KB")
            } else if size < 1024 * 1024 * 1000 {
                (format!("{mb:.2}"), "MB")
            } else {
                (format!("{gb:.2}"), "GB")
            }
        }
        SpeedUnit::Kbps => {
            if cfg.speed_short_mode {
                let v = if size < 1024 * 10 { format!("{kb:.1}") } else { format!("{kb:.0}") };
                (v, if cfg.hide_unit { "" } else { "K" })
            } else {
                let v = if size < 1024 * 10 { format!("{kb:.2}") } else { format!("{kb:.1}") };
                (v, if cfg.hide_unit { "" } else { "KB" })
            }
        }
        SpeedUnit::Mbps => {
            if cfg.speed_short_mode {
                (format!("{mb:.1}"), if cfg.hide_unit { "" } else { "M" })
            } else {
                (format!("{mb:.2}"), if cfg.hide_unit { "" } else { "MB" })
            }
        }
    };
    let mut s = if cfg.separate_value_unit_with_space && !cfg.hide_unit {
        format!("{value} {unit}")
    } else {
        format!("{value}{unit}")
    };
    if !cfg.unit_byte {
        if cfg.speed_short_mode && !cfg.hide_unit {
            s.push('b');
        } else {
            s = s.replace('B', "b");
        }
    }
    s
}

/// `CCommon::DataSizeToString(size, with_space)`.
pub fn data_size_to_string(size: u64, with_space: bool) -> String {
    let s = if size < 1024 * 10 {
        format!("{:.2} KB", size as f64 / 1024.0)
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.2} MB", size as f64 / 1024.0 / 1024.0)
    } else if size < 1024u64 * 1024 * 1024 * 1024 {
        format!("{:.2} GB", size as f64 / 1024.0 / 1024.0 / 1024.0)
    } else {
        format!("{:.2} TB", size as f64 / 1024.0 / 1024.0 / 1024.0 / 1024.0)
    };
    if with_space { s } else { s.replace(' ', "") }
}

/// `CCommon::KBytesToString`.
pub fn kbytes_to_string(kb: u64) -> String {
    if kb < 1024 {
        format!("{kb} KB")
    } else if kb < 1024 * 1024 {
        format!("{:.2} MB", kb as f64 / 1024.0)
    } else if kb < 1024 * 1024 * 1024 {
        format!("{:.2} GB", kb as f64 / 1024.0 / 1024.0)
    } else {
        format!("{:.2} TB", kb as f64 / 1024.0 / 1024.0 / 1024.0)
    }
}

pub fn temperature_to_string(t: f32, cfg: &TaskbarSettings) -> String {
    let mut s = if t <= 0.0 { "--".to_string() } else { format!("{}", t as i32) };
    if cfg.separate_value_unit_with_space {
        s.push(' ');
    }
    s.push_str("°C");
    s
}

pub fn usage_to_string(usage: i32, cfg: &TaskbarSettings) -> String {
    let mut s = if usage < 0 { "--".to_string() } else { usage.to_string() };
    if !cfg.hide_percent {
        if cfg.separate_value_unit_with_space {
            s.push(' ');
        }
        s.push('%');
    }
    s
}

pub fn freq_to_string(freq: f32, cfg: &TaskbarSettings) -> String {
    let mut s = if freq < 0.0 { "--".to_string() } else { format!("{freq:.2}") };
    if cfg.separate_value_unit_with_space {
        s.push(' ');
    }
    s.push_str("GHz");
    s
}

/// Value text shown on the taskbar for a built-in item (`GetItemValueText(false)`).
pub fn item_value_text(item: DisplayItem, snap: &Snapshot, cfg: &TaskbarSettings) -> String {
    match item {
        DisplayItem::Up | DisplayItem::Down | DisplayItem::TotalSpeed => {
            let suffix = !cfg.hide_unit || cfg.speed_unit == SpeedUnit::Auto;
            let v = match item {
                DisplayItem::Up => snap.out_speed,
                DisplayItem::Down => snap.in_speed,
                _ => snap.in_speed + snap.out_speed,
            };
            let mut s = speed_to_string(v, cfg);
            if suffix {
                s.push_str("/s");
            }
            s
        }
        DisplayItem::Cpu => usage_to_string(snap.cpu_usage, cfg),
        DisplayItem::Memory => match cfg.memory_display {
            MemoryDisplay::MemoryUsed => data_size_to_string(snap.used_memory_kb * 1024, cfg.separate_value_unit_with_space),
            MemoryDisplay::MemoryAvailable => data_size_to_string(
                snap.total_memory_kb.saturating_sub(snap.used_memory_kb) * 1024,
                cfg.separate_value_unit_with_space,
            ),
            MemoryDisplay::UsagePercentage => usage_to_string(snap.memory_usage, cfg),
        },
        DisplayItem::GpuUsage => usage_to_string(snap.gpu_usage, cfg),
        DisplayItem::HddUsage => usage_to_string(snap.hdd_usage, cfg),
        DisplayItem::CpuTemp => temperature_to_string(snap.cpu_temperature, cfg),
        DisplayItem::GpuTemp => temperature_to_string(snap.gpu_temperature, cfg),
        DisplayItem::HddTemp => temperature_to_string(snap.hdd_temperature, cfg),
        DisplayItem::MainBoardTemp => temperature_to_string(snap.main_board_temperature, cfg),
        DisplayItem::CpuFreq => freq_to_string(snap.cpu_freq, cfg),
        DisplayItem::TodayTraffic => kbytes_to_string((snap.today_up_traffic + snap.today_down_traffic) / 1024),
    }
}

/// Width sample text (`GetItemValueSampleText(false)`).
pub fn item_sample_text(item: DisplayItem, snap: &Snapshot, cfg: &TaskbarSettings) -> String {
    match item {
        DisplayItem::Up | DisplayItem::Down | DisplayItem::TotalSpeed => {
            let digits = "8".repeat(cfg.digits_number.max(1) as usize);
            let hide_unit = cfg.hide_unit && cfg.speed_unit != SpeedUnit::Auto;
            let mut s = if cfg.speed_short_mode {
                if hide_unit { format!("{digits}.") } else { format!("{digits}.M/s") }
            } else if hide_unit {
                format!("{digits}.8")
            } else {
                format!("{digits}.8MB/s")
            };
            if !hide_unit && cfg.separate_value_unit_with_space {
                s.push(' ');
            }
            if cfg.speed_short_mode && !cfg.unit_byte && !cfg.hide_unit {
                s.push('b');
            }
            s
        }
        DisplayItem::Cpu | DisplayItem::Memory | DisplayItem::GpuUsage | DisplayItem::HddUsage => {
            let mut s = "100".to_string();
            if !cfg.hide_percent {
                s.push_str(if cfg.separate_value_unit_with_space { " %" } else { "%" });
            }
            if item == DisplayItem::Memory
                && matches!(cfg.memory_display, MemoryDisplay::MemoryUsed | MemoryDisplay::MemoryAvailable)
            {
                s = data_size_to_string(snap.total_memory_kb * 1024, cfg.separate_value_unit_with_space);
            }
            s
        }
        DisplayItem::CpuTemp | DisplayItem::GpuTemp | DisplayItem::HddTemp | DisplayItem::MainBoardTemp => {
            if cfg.separate_value_unit_with_space { "99 °C".into() } else { "99°C".into() }
        }
        DisplayItem::CpuFreq => {
            if cfg.separate_value_unit_with_space { "1.00 GHz".into() } else { "1.00GHz".into() }
        }
        DisplayItem::TodayTraffic => {
            if cfg.separate_value_unit_with_space { "999.99 MB".into() } else { "999.99MB".into() }
        }
    }
}

/// `CCommon::IntToString(n, thousand_separator=true, is_unsigned)`.
pub fn int_with_separator(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
