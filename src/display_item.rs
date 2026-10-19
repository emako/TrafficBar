use crate::i18n::tr;

/// Built-in taskbar items. The discriminants match `DisplayItem` in the original, because they are
/// persisted as bit flags (`tbar_display_item`) and as positions in `item_order`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum DisplayItem {
    Up = 0,
    Down = 1,
    Cpu = 2,
    Memory = 3,
    GpuUsage = 4,
    CpuTemp = 5,
    GpuTemp = 6,
    HddTemp = 7,
    MainBoardTemp = 8,
    HddUsage = 9,
    TotalSpeed = 10,
    CpuFreq = 11,
    TodayTraffic = 12,
}

pub const ALL_BUILTIN: [DisplayItem; 13] = [
    DisplayItem::Up,
    DisplayItem::Down,
    DisplayItem::Cpu,
    DisplayItem::Memory,
    DisplayItem::GpuUsage,
    DisplayItem::CpuTemp,
    DisplayItem::GpuTemp,
    DisplayItem::HddTemp,
    DisplayItem::MainBoardTemp,
    DisplayItem::HddUsage,
    DisplayItem::TotalSpeed,
    DisplayItem::CpuFreq,
    DisplayItem::TodayTraffic,
];

impl DisplayItem {
    pub fn is_temperature(self) -> bool {
        matches!(
            self,
            DisplayItem::CpuTemp
                | DisplayItem::GpuTemp
                | DisplayItem::HddTemp
                | DisplayItem::MainBoardTemp
        )
    }

    pub fn is_net_speed(self) -> bool {
        matches!(
            self,
            DisplayItem::Up | DisplayItem::Down | DisplayItem::TotalSpeed
        )
    }

    /// Temperatures come from LibreHardwareMonitor, which needs administrator rights.
    pub fn requires_admin(self) -> bool {
        self.is_temperature()
    }

    pub fn hardware(self) -> Option<HardwareItem> {
        match self {
            DisplayItem::CpuTemp => Some(HardwareItem::Cpu),
            DisplayItem::GpuTemp => Some(HardwareItem::Gpu),
            DisplayItem::HddTemp => Some(HardwareItem::Hdd),
            DisplayItem::MainBoardTemp => Some(HardwareItem::MainBoard),
            _ => None,
        }
    }

    pub fn name(self) -> String {
        tr(match self {
            DisplayItem::Up => "IDS_UPLOAD",
            DisplayItem::Down => "IDS_DOWNLOAD",
            DisplayItem::TotalSpeed => "IDS_TOTAL_NET_SPEED",
            DisplayItem::Cpu => "IDS_CPU_USAGE",
            DisplayItem::Memory => "IDS_MEMORY_USAGE",
            DisplayItem::GpuUsage => "IDS_GPU_USAGE",
            DisplayItem::CpuTemp => "IDS_CPU_TEMPERATURE",
            DisplayItem::GpuTemp => "IDS_GPU_TEMPERATURE",
            DisplayItem::HddTemp => "IDS_HDD_TEMPERATURE",
            DisplayItem::MainBoardTemp => "IDS_MAINBOARD_TEMPERATURE",
            DisplayItem::HddUsage => "IDS_HDD_USAGE",
            DisplayItem::CpuFreq => "IDS_CPU_FREQ",
            DisplayItem::TodayTraffic => "IDS_TRAFFIC_USED",
        })
    }

    /// Default label text of the taskbar window (`CommonDisplayItem::DefaultString(false)`).
    pub fn default_label(self) -> String {
        match self {
            DisplayItem::Up => "↑: ".into(),
            DisplayItem::Down => "↓: ".into(),
            DisplayItem::TotalSpeed => "↑↓: ".into(),
            DisplayItem::TodayTraffic => tr("IDS_TRAFFIC_USED") + ": ",
            DisplayItem::Cpu | DisplayItem::CpuTemp => "CPU: ".into(),
            DisplayItem::CpuFreq => tr("IDS_CPU_FREQ") + ": ",
            DisplayItem::Memory => tr("IDS_MEMORY_DISP") + ": ",
            DisplayItem::GpuUsage | DisplayItem::GpuTemp => tr("IDS_GPU_DISP") + ": ",
            DisplayItem::HddTemp | DisplayItem::HddUsage => tr("IDS_HDD_DISP") + ": ",
            DisplayItem::MainBoardTemp => tr("IDS_MAINBOARD_DISP") + ": ",
        }
    }

    pub fn ini_key(self) -> &'static str {
        match self {
            DisplayItem::Up => "up_string",
            DisplayItem::Down => "down_string",
            DisplayItem::Cpu => "cpu_string",
            DisplayItem::Memory => "memory_string",
            DisplayItem::GpuUsage => "gpu_string",
            DisplayItem::CpuTemp => "cpu_temp_string",
            DisplayItem::GpuTemp => "gpu_temp_string",
            DisplayItem::HddTemp => "hdd_temp_string",
            DisplayItem::MainBoardTemp => "main_board_temp_string",
            DisplayItem::HddUsage => "hdd_string",
            DisplayItem::TotalSpeed => "total_speed_string",
            DisplayItem::CpuFreq => "cpu_freq_string",
            DisplayItem::TodayTraffic => "today_traffic_string",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum HardwareItem {
    Cpu = 1,
    Gpu = 2,
    Hdd = 4,
    MainBoard = 8,
}

/// A built-in item or a plugin item (identified by `IPluginItem::GetItemId`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ItemKey {
    Builtin(DisplayItem),
    Plugin(String),
}

/// Bit set of built-in items (`DisplayItemSet`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DisplayItemSet(pub u32);

impl DisplayItemSet {
    pub fn contains(&self, item: DisplayItem) -> bool {
        self.0 & (1 << item as u32) != 0
    }
    pub fn set(&mut self, item: DisplayItem, on: bool) {
        if on {
            self.0 |= 1 << item as u32;
        } else {
            self.0 &= !(1 << item as u32);
        }
    }
    pub fn is_empty(&self) -> bool {
        self.0 & 0x1FFF == 0
    }
    pub fn from_int(v: i32) -> Self {
        DisplayItemSet(v as u32 & 0x1FFF)
    }
    pub fn to_int(&self) -> i32 {
        (self.0 & 0x1FFF) as i32
    }
}

/// Every taskbar item in default order: built-ins first, then plugin items in load order
/// (`CPluginManager::AllDisplayItemsWithPlugins`).
pub fn all_items(plugin_ids: &[String]) -> Vec<ItemKey> {
    ALL_BUILTIN
        .iter()
        .map(|d| ItemKey::Builtin(*d))
        .chain(plugin_ids.iter().map(|id| ItemKey::Plugin(id.clone())))
        .collect()
}

/// `CTaskbarItemOrderHelper` with `displayed_only == false`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemOrder {
    pub order: Vec<i32>,
}

impl ItemOrder {
    pub fn from_string(s: &str, item_count: usize) -> Self {
        let mut o = ItemOrder {
            order: s
                .split(',')
                .filter(|p| !p.trim().is_empty())
                .map(|p| crate::config::ini::atoi(p))
                .collect(),
        };
        o.normalize(item_count);
        o
    }

    pub fn to_string(&self) -> String {
        self.order
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn normalize(&mut self, item_count: usize) {
        let n = item_count as i32;
        self.order.retain(|i| *i >= 0 && *i < n);
        let mut seen = std::collections::HashSet::new();
        self.order.retain(|i| seen.insert(*i));
        for i in 0..n {
            if !self.order.contains(&i) {
                self.order.push(i);
            }
        }
    }

    pub fn ordered(&self, all: &[ItemKey]) -> Vec<ItemKey> {
        self.order
            .iter()
            .filter_map(|i| all.get(*i as usize).cloned())
            .collect()
    }
}
