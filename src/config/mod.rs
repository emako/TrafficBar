pub mod ini;

use std::collections::HashMap;
use std::path::PathBuf;

use crate::display_item::{all_items, DisplayItem, DisplayItemSet, HardwareItem, ItemKey, ItemOrder, ALL_BUILTIN};
use crate::i18n;
use crate::win;
use ini::Ini;

pub const MONITOR_TIME_SPAN_MIN: i32 = 200;
pub const MONITOR_TIME_SPAN_MAX: i32 = 30000;
pub const TASKBAR_DEFAULT_STYLE_NUM: usize = 4;
pub const TASKBAR_DEFAULT_LIGHT_STYLE_INDEX: i32 = 3;
const TASKBAR_TRANSPARENT_COLOR1: u32 = rgb(52, 28, 41);
const TASKBAR_TRANSPARENT_COLOR2: u32 = rgb(38, 67, 55);

/// COLORREF layout (0x00BBGGRR).
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}

pub fn color_r(c: u32) -> u8 {
    (c & 0xFF) as u8
}
pub fn color_g(c: u32) -> u8 {
    ((c >> 8) & 0xFF) as u8
}
pub fn color_b(c: u32) -> u8 {
    ((c >> 16) & 0xFF) as u8
}

#[derive(Clone, Debug, PartialEq)]
pub struct FontInfo {
    pub name: String,
    pub size: i32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike_out: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpeedUnit {
    Auto = 0,
    Kbps = 1,
    Mbps = 2,
}

impl SpeedUnit {
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => SpeedUnit::Kbps,
            2 => SpeedUnit::Mbps,
            _ => SpeedUnit::Auto,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemoryDisplay {
    UsagePercentage = 0,
    MemoryUsed = 1,
    MemoryAvailable = 2,
}

impl MemoryDisplay {
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => MemoryDisplay::MemoryUsed,
            2 => MemoryDisplay::MemoryAvailable,
            _ => MemoryDisplay::UsagePercentage,
        }
    }
}

/// Values match `DoubleClickAction` so existing settings keep their meaning.
/// `ShowMoreInfo` and `ChangeSkin` belong to the removed main window and fall back to `None`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DoubleClickAction {
    ConnectionInfo = 0,
    HistoryTraffic = 1,
    Options = 3,
    TaskManager = 4,
    SpecificApp = 5,
    None = 7,
}

impl DoubleClickAction {
    pub fn from_i32(v: i32) -> Self {
        match v {
            0 => DoubleClickAction::ConnectionInfo,
            1 => DoubleClickAction::HistoryTraffic,
            3 => DoubleClickAction::Options,
            4 => DoubleClickAction::TaskManager,
            5 => DoubleClickAction::SpecificApp,
            _ => DoubleClickAction::None,
        }
    }

    pub const ALL: [DoubleClickAction; 6] = [
        DoubleClickAction::ConnectionInfo,
        DoubleClickAction::HistoryTraffic,
        DoubleClickAction::Options,
        DoubleClickAction::TaskManager,
        DoubleClickAction::SpecificApp,
        DoubleClickAction::None,
    ];

    pub fn label(self) -> String {
        i18n::tr(match self {
            DoubleClickAction::ConnectionInfo => "IDS_OPEN_CONNECTION_DETIAL",
            DoubleClickAction::HistoryTraffic => "IDS_OPEN_HISTORICAL_TRAFFIC",
            DoubleClickAction::Options => "IDS_OPEN_OPTION_SETTINGS",
            DoubleClickAction::TaskManager => "IDS_OPEN_TASK_MANAGER",
            DoubleClickAction::SpecificApp => "IDS_SPECIFIC_APP",
            DoubleClickAction::None => "IDS_NONE",
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ItemColor {
    pub label: u32,
    pub value: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HistoryViewType {
    Day = 0,
    Week = 1,
    Month = 2,
    Quarter = 3,
    Year = 4,
}

impl HistoryViewType {
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => HistoryViewType::Week,
            2 => HistoryViewType::Month,
            3 => HistoryViewType::Quarter,
            4 => HistoryViewType::Year,
            _ => HistoryViewType::Day,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskbarSettings {
    pub back_color: u32,
    pub transparent_color: u32,
    pub status_bar_color: u32,
    pub text_colors: HashMap<ItemKey, ItemColor>,
    pub specify_each_item_color: bool,
    pub font: FontInfo,
    pub disp_str: HashMap<ItemKey, String>,
    pub speed_short_mode: bool,
    pub separate_value_unit_with_space: bool,
    pub show_tool_tip: bool,
    pub memory_display: MemoryDisplay,
    pub unit_byte: bool,
    pub speed_unit: SpeedUnit,
    pub hide_unit: bool,
    pub hide_percent: bool,
    pub double_click_action: DoubleClickAction,
    pub double_click_exe: String,

    pub auto_adapt_light_theme: bool,
    pub dark_default_style: i32,
    pub light_default_style: i32,
    pub auto_save_taskbar_color_settings_to_preset: bool,

    pub item_order: ItemOrder,
    pub display_item: DisplayItemSet,
    pub plugin_display_item: Vec<String>,

    pub show_taskbar_wnd_in_secondary_display: bool,
    pub secondary_display_index: i32,
    pub value_right_align: bool,
    pub digits_number: i32,
    pub horizontal_arrange: bool,
    pub show_status_bar: bool,
    pub tbar_wnd_on_left: bool,
    pub tbar_wnd_snap: bool,
    pub cm_graph_type: bool,
    pub show_graph_dashed_box: bool,
    pub item_space: i32,
    pub vertical_margin: i32,
    pub window_offset_top: i32,
    pub window_offset_left: i32,
    pub avoid_overlap_with_widgets: bool,
    pub taskbar_left_space_win11: i32,
    pub taskbar_right_space_win11: i32,
    pub show_netspeed_figure: bool,
    pub netspeed_figure_max_value: i32,
    pub netspeed_figure_max_value_unit: i32,
    pub graph_color_following_system: bool,
    pub enable_colorful_emoji: bool,
}

impl TaskbarSettings {
    /// `TaskBarSettingData::IsTaskbarTransparent`.
    pub fn is_transparent(&self, rt: &Runtime) -> bool {
        if rt.light_theme || rt.version.is_windows8_or_81() || rt.win11_taskbar {
            self.transparent_color == self.back_color
        } else {
            self.transparent_color == 0
        }
    }

    pub fn set_transparent(&mut self, transparent: bool, rt: &Runtime) {
        if transparent {
            if rt.light_theme || rt.version.is_windows8_or_81() || rt.win11_taskbar {
                transparent_color_convert(&mut self.back_color);
                self.transparent_color = self.back_color;
            } else {
                self.transparent_color = 0;
            }
        } else if self.back_color != TASKBAR_TRANSPARENT_COLOR1 {
            self.transparent_color = TASKBAR_TRANSPARENT_COLOR1;
        } else {
            self.transparent_color = TASKBAR_TRANSPARENT_COLOR2;
        }
    }

    pub fn netspeed_figure_max_bytes(&self) -> u64 {
        let v = self.netspeed_figure_max_value.max(0) as u64;
        if self.netspeed_figure_max_value_unit == 0 {
            v * 1024
        } else {
            v * 1024 * 1024
        }
    }

    /// `TaskBarSettingData::GetUsageGraphColor`.
    pub fn usage_graph_color(&self, rt: &Runtime) -> u32 {
        if self.graph_color_following_system {
            let (h, s, _) = rgb_to_hsl(rt.theme_color);
            let l = if rt.light_theme { 0.7 } else { 0.4 };
            hsl_to_rgb(h, s, l)
        } else {
            self.status_bar_color
        }
    }

    pub fn item_color(&self, key: &ItemKey) -> ItemColor {
        if self.specify_each_item_color {
            self.text_colors.get(key).copied().unwrap_or_default()
        } else {
            let first = self
                .text_colors
                .get(&ItemKey::Builtin(DisplayItem::Up))
                .copied()
                .unwrap_or_default();
            ItemColor {
                label: first.label,
                value: first.label,
            }
        }
    }

    pub fn label(&self, key: &ItemKey) -> String {
        self.disp_str.get(key).cloned().unwrap_or_default()
    }

    pub fn is_item_displayed(&self, key: &ItemKey) -> bool {
        match key {
            ItemKey::Builtin(d) => self.display_item.contains(*d),
            ItemKey::Plugin(id) => self.plugin_display_item.iter().any(|p| p == id),
        }
    }

    pub fn set_item_displayed(&mut self, key: &ItemKey, on: bool) {
        match key {
            ItemKey::Builtin(d) => self.display_item.set(*d, on),
            ItemKey::Plugin(id) => {
                self.plugin_display_item.retain(|p| p != id);
                if on {
                    self.plugin_display_item.push(id.clone());
                    self.plugin_display_item.sort();
                }
            }
        }
    }

    pub fn valid(&mut self) {
        self.item_space = self.item_space.clamp(0, 32);
        self.vertical_margin = self.vertical_margin.clamp(-10, 10);
        self.window_offset_top = self.window_offset_top.clamp(-20, 20);
        self.window_offset_left = self.window_offset_left.clamp(-800, 800);
        self.digits_number = self.digits_number.clamp(3, 7);
        self.font.size = self.font.size.clamp(5, 72);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotifyTip {
    pub enable: bool,
    pub tip_value: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralSettings {
    pub check_update_when_start: bool,
    /// Ask for administrator rights at startup. A denied UAC prompt keeps the process unelevated.
    pub always_run_as_admin: bool,
    pub update_source: i32,
    pub auto_run: bool,
    pub auto_run_by_task_scheduler: bool,
    pub show_notify_icon: bool,
    pub traffic_tip_enable: bool,
    pub traffic_tip_value: i32,
    pub traffic_tip_unit: i32,
    pub memory_usage_tip: NotifyTip,
    pub cpu_temp_tip: NotifyTip,
    pub gpu_temp_tip: NotifyTip,
    pub hdd_temp_tip: NotifyTip,
    pub mainboard_temp_tip: NotifyTip,
    /// bcp-47 tag, empty to follow the system.
    pub language: String,
    pub show_all_interface: bool,
    /// 0: CPU time, 1: PDH.
    pub cpu_usage_acquire_method: i32,
    pub portable_mode: bool,
    pub monitor_time_span: i32,
    pub hard_disk_name: String,
    pub cpu_core_name: String,
    pub hardware_monitor_item: u32,
    pub connections_hide: Vec<String>,
}

impl GeneralSettings {
    pub fn is_hardware_enabled(&self, item: HardwareItem) -> bool {
        self.hardware_monitor_item & item as u32 != 0
    }
    pub fn set_hardware_enabled(&mut self, item: HardwareItem, on: bool) {
        if on {
            self.hardware_monitor_item |= item as u32;
        } else {
            self.hardware_monitor_item &= !(item as u32);
        }
    }
    pub fn any_hardware_enabled(&self) -> bool {
        self.hardware_monitor_item & 0xF != 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfigData {
    pub auto_select: bool,
    pub select_all: bool,
    pub connection_name: String,
    pub use_log_scale: bool,
    pub sunday_first: bool,
    pub view_type: HistoryViewType,
    pub plugin_disabled: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct TaskbarStyle {
    pub text_colors: HashMap<ItemKey, ItemColor>,
    pub back_color: u32,
    pub transparent_color: u32,
    pub status_bar_color: u32,
    pub specify_each_item_color: bool,
}

impl TaskbarStyle {
    /// `CTaskbarDefaultStyle::IsTaskBarStyleDataValid`.
    pub fn is_valid(&self) -> bool {
        self.text_colors
            .values()
            .any(|c| c.label != self.back_color || c.value != self.back_color)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OtherSettings {
    pub notify_interval: i32,
    pub debug_log: bool,
    pub last_light_mode: bool,
}

/// Values that are detected at runtime and influence how settings are interpreted.
#[derive(Clone, Debug)]
pub struct Runtime {
    pub light_theme: bool,
    pub win11_taskbar: bool,
    pub version: win::WinVersion,
    pub theme_color: u32,
    pub elevated: bool,
    pub web_experience_detected: bool,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub taskbar: TaskbarSettings,
    pub general: GeneralSettings,
    pub cfg: ConfigData,
    pub styles: Vec<TaskbarStyle>,
    pub other: OtherSettings,
    pub config_dir: PathBuf,
}

pub fn transparent_color_convert(color: &mut u32) {
    if *color == 0 {
        return;
    }
    let r = color_r(*color);
    let g = color_g(*color);
    let mut b = color_b(*color);
    if r == b {
        if b >= 255 {
            b -= 1;
        } else {
            b += 1;
        }
        *color = rgb(r, g, b);
    }
}

/// Resolves where config.ini / history_traffic.dat live (`LoadGlobalConfig`).
pub fn resolve_config_dir() -> (PathBuf, bool, bool) {
    let program_dir = win::program_dir();
    let global = Ini::load(&program_dir.join("global_cfg.ini"));
    let mut portable = global.get_bool("config", "portable_mode", false);
    let writable = is_dir_writable(&program_dir);
    if !writable {
        portable = false;
    }
    let dir = if portable { program_dir } else { win::appdata_dir() };
    let _ = std::fs::create_dir_all(&dir);
    (dir, portable, writable)
}

pub fn save_global_config(portable: bool) -> bool {
    let path = win::program_dir().join("global_cfg.ini");
    let mut ini = Ini::load(&path);
    ini.set_bool("config", "portable_mode", portable);
    ini.save().is_ok()
}

fn is_dir_writable(dir: &std::path::Path) -> bool {
    let probe = dir.join(".tmbar_write_test");
    let ok = std::fs::write(&probe, b"1").is_ok();
    let _ = std::fs::remove_file(&probe);
    let temp = std::env::temp_dir();
    ok && !dir.starts_with(&temp)
}

pub fn read_language(config_dir: &std::path::Path) -> String {
    let ini = Ini::load(&config_dir.join("config.ini"));
    let s = ini.get_string("general", "language", "");
    s.split('|').next().unwrap_or("").to_string()
}

pub fn read_always_run_as_admin(config_dir: &std::path::Path) -> bool {
    let ini = Ini::load(&config_dir.join("config.ini"));
    ini.get_bool("general", "always_run_as_admin", false)
}

fn load_colors(ini: &Ini, section: &str, key: &str, all: &[ItemKey], default_str: &str) -> HashMap<ItemKey, ItemColor> {
    let raw = ini.get_string(section, key, default_str);
    let parts: Vec<u32> = raw
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| ini::atoi64(s) as u32)
        .collect();
    let default_color = ini::atoi64(default_str) as u32;
    let mut map = HashMap::new();
    for (i, item) in all.iter().enumerate() {
        let label = parts.get(i * 2).copied().unwrap_or(default_color);
        let value = parts.get(i * 2 + 1).copied().unwrap_or(default_color);
        map.insert(item.clone(), ItemColor { label, value });
    }
    map
}

fn save_colors(ini: &mut Ini, section: &str, key: &str, all: &[ItemKey], colors: &HashMap<ItemKey, ItemColor>) {
    let mut s = String::new();
    for item in all {
        let c = colors.get(item).copied().unwrap_or_default();
        s.push_str(&format!("{},{},", c.label, c.value));
    }
    ini.set_string(section, key, &s);
}

impl Settings {
    pub fn load(config_dir: PathBuf, portable: bool, plugin_items: &[(String, String)], rt: &Runtime) -> Settings {
        let ini = Ini::load(&config_dir.join("config.ini"));
        let plugin_ids: Vec<String> = plugin_items.iter().map(|(id, _)| id.clone()).collect();
        let all = all_items(&plugin_ids);
        let light = rt.light_theme;

        let (dft_back, dft_transparent, dft_status, dft_text) = if light {
            (0x00D3D2D2u32, 0x00D3D2D2u32, 0x00A5A5A5u32, 0u32)
        } else if rt.version.is_windows8_or_81() {
            (rgb(0, 0, 1), rgb(0, 0, 1), 0x005A5A5A, 0x00FFFFFF)
        } else {
            (0, 0, 0x005A5A5A, 0x00FFFFFF)
        };

        let system_dir = win::system_dir();
        let taskmgr = system_dir.join("Taskmgr.exe").to_string_lossy().into_owned();

        let general = GeneralSettings {
            check_update_when_start: ini.get_bool("general", "check_update_when_start", true),
            always_run_as_admin: ini.get_bool("general", "always_run_as_admin", false),
            update_source: ini.get_int("general", "update_source", if i18n::is_zh() { 1 } else { 0 }),
            auto_run: false,
            auto_run_by_task_scheduler: ini.get_bool("general", "auto_run_by_task_scheduler", false),
            show_notify_icon: true,
            traffic_tip_enable: ini.get_bool("notify_tip", "traffic_tip_enable", false),
            traffic_tip_value: ini.get_int("notify_tip", "traffic_tip_value", 200),
            traffic_tip_unit: ini.get_int("notify_tip", "traffic_tip_unit", 0),
            memory_usage_tip: NotifyTip {
                enable: ini.get_bool("notify_tip", "memory_usage_tip_enable", false),
                tip_value: ini.get_int("notify_tip", "memory_tip_value", 80),
            },
            cpu_temp_tip: NotifyTip {
                enable: ini.get_bool("notify_tip", "cpu_temperature_tip_enable", false),
                tip_value: ini.get_int("notify_tip", "cpu_temperature_tip_value", 80),
            },
            gpu_temp_tip: NotifyTip {
                enable: ini.get_bool("notify_tip", "gpu_temperature_tip_enable", false),
                tip_value: ini.get_int("notify_tip", "gpu_temperature_tip_value", 80),
            },
            hdd_temp_tip: NotifyTip {
                enable: ini.get_bool("notify_tip", "hdd_temperature_tip_enable", false),
                tip_value: ini.get_int("notify_tip", "hdd_temperature_tip_value", 80),
            },
            mainboard_temp_tip: NotifyTip {
                enable: ini.get_bool("notify_tip", "mainboard_temperature_tip_enable", false),
                tip_value: ini.get_int("notify_tip", "mainboard_temperature_tip_value", 80),
            },
            language: ini
                .get_string("general", "language", "")
                .split('|')
                .next()
                .unwrap_or("")
                .to_string(),
            show_all_interface: ini.get_bool("general", "show_all_interface", false),
            cpu_usage_acquire_method: ini.get_int("general", "cpu_usage_acquire_method", 1),
            portable_mode: portable,
            monitor_time_span: {
                let v = ini.get_int("general", "monitor_time_span", 1000);
                if (MONITOR_TIME_SPAN_MIN..=MONITOR_TIME_SPAN_MAX).contains(&v) { v } else { 1000 }
            },
            hard_disk_name: ini.get_string("general", "hard_disk_name", ""),
            cpu_core_name: ini.get_string("general", "cpu_core_name", "Core Average"),
            hardware_monitor_item: ini.get_u32("general", "hardware_monitor_item", 0),
            connections_hide: ini.get_string_list("general", "connections_hide"),
        };

        let mut back_color = ini.get_u32("task_bar", "task_bar_back_color", dft_back);
        let mut transparent_color = ini.get_u32("task_bar", "transparent_color", dft_transparent);
        let mut text_colors = load_colors(&ini, "task_bar", "task_bar_text_color", &all, &dft_text.to_string());

        let mut display_item = DisplayItemSet::from_int(ini.get_int(
            "task_bar",
            "tbar_display_item",
            DisplayItemSet((1 << DisplayItem::Up as u32) | (1 << DisplayItem::Down as u32)).to_int(),
        ));
        for d in ALL_BUILTIN {
            if let Some(hw) = d.hardware() {
                if !general.is_hardware_enabled(hw) {
                    display_item.set(d, false);
                }
            }
        }

        let first_label = text_colors
            .get(&ItemKey::Builtin(DisplayItem::Up))
            .map(|c| c.label)
            .unwrap_or(dft_text);
        if back_color == 0 && first_label == 0 {
            back_color = dft_back;
            if let Some(c) = text_colors.get_mut(&ItemKey::Builtin(DisplayItem::Up)) {
                c.label = dft_text;
            }
        }

        let default_font_name = i18n::default_font();
        let style_bits = ini.get_int("task_bar", "font_style", 0);
        let font = FontInfo {
            name: ini.get_string("task_bar", "font_name", &default_font_name),
            size: ini.get_int("task_bar", "font_size", 9),
            bold: style_bits & 1 != 0,
            italic: style_bits & 2 != 0,
            underline: style_bits & 4 != 0,
            strike_out: style_bits & 8 != 0,
        };

        let mut disp_str = HashMap::new();
        for d in ALL_BUILTIN {
            disp_str.insert(ItemKey::Builtin(d), ini.get_string("task_bar", d.ini_key(), &d.default_label()));
        }
        for (id, label) in plugin_items {
            disp_str.insert(
                ItemKey::Plugin(id.clone()),
                ini.get_string("plugin_display_str_taskbar_window", id, label),
            );
        }

        let mut taskbar = TaskbarSettings {
            back_color,
            transparent_color,
            status_bar_color: ini.get_u32("task_bar", "status_bar_color", dft_status),
            text_colors: HashMap::new(),
            specify_each_item_color: ini.get_bool("task_bar", "specify_each_item_color", false),
            font,
            disp_str,
            speed_short_mode: ini.get_bool("task_bar", "task_bar_speed_short_mode", true),
            separate_value_unit_with_space: ini.get_bool("task_bar", "separate_value_unit_with_space", true),
            show_tool_tip: ini.get_bool("task_bar", "show_tool_tip", true),
            memory_display: MemoryDisplay::from_i32(ini.get_int("task_bar", "memory_display", 0)),
            unit_byte: ini.get_bool("task_bar", "unit_byte", true),
            speed_unit: SpeedUnit::from_i32(ini.get_int("task_bar", "task_bar_speed_unit", 0)),
            hide_unit: ini.get_bool("task_bar", "task_bar_hide_unit", false),
            hide_percent: ini.get_bool("task_bar", "task_bar_hide_percent", false),
            double_click_action: DoubleClickAction::from_i32(ini.get_int("task_bar", "double_click_action", 0)),
            double_click_exe: ini.get_string("task_bar", "double_click_exe", &taskmgr),
            auto_adapt_light_theme: rt.version.is_windows10_or_later()
                && ini.get_bool("task_bar", "auto_adapt_light_theme", false),
            dark_default_style: ini.get_int("task_bar", "dark_default_style", 0),
            light_default_style: ini.get_int("task_bar", "light_default_style", TASKBAR_DEFAULT_LIGHT_STYLE_INDEX),
            auto_save_taskbar_color_settings_to_preset: ini.get_bool(
                "task_bar",
                "auto_save_taskbar_color_settings_to_preset",
                true,
            ),
            item_order: ItemOrder::from_string(&ini.get_string("task_bar", "item_order", ""), all.len()),
            display_item,
            plugin_display_item: ini
                .get_string("task_bar", "plugin_display_item", "")
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect(),
            show_taskbar_wnd_in_secondary_display: ini.get_bool("task_bar", "show_taskbar_wnd_in_secondary_display", false),
            secondary_display_index: ini.get_int("task_bar", "secondary_display_index", 0),
            value_right_align: ini.get_bool("task_bar", "value_right_align", true),
            digits_number: ini.get_int("task_bar", "digits_number", 4),
            horizontal_arrange: ini.get_bool("task_bar", "horizontal_arrange", false),
            show_status_bar: ini.get_bool("task_bar", "show_status_bar", true),
            tbar_wnd_on_left: ini.get_bool("task_bar", "task_bar_wnd_on_left", false),
            tbar_wnd_snap: ini.get_bool("task_bar", "task_bar_wnd_snap", false),
            cm_graph_type: ini.get_bool("task_bar", "cm_graph_type", true),
            show_graph_dashed_box: ini.get_bool("task_bar", "show_graph_dashed_box", false),
            item_space: ini.get_int("task_bar", "item_space", 8),
            vertical_margin: ini.get_int("task_bar", "vertical_margin", 0),
            window_offset_top: ini.get_int("task_bar", "window_offset_top", 0),
            window_offset_left: ini.get_int("task_bar", "window_offset_left", 0),
            avoid_overlap_with_widgets: ini.get_bool("task_bar", "avoid_overlap_with_widgets", false),
            taskbar_left_space_win11: ini.get_int("task_bar", "taskbar_left_space_win11", 160),
            taskbar_right_space_win11: ini.get_int("task_bar", "taskbar_right_space_win11", 280),
            show_netspeed_figure: ini.get_bool("task_bar", "show_netspeed_figure", true),
            netspeed_figure_max_value: ini.get_int("task_bar", "netspeed_figure_max_value", 10),
            netspeed_figure_max_value_unit: ini.get_int("task_bar", "netspeed_figure_max_value_unit", 1),
            graph_color_following_system: ini.get_bool("task_bar", "graph_color_following_system", true),
            enable_colorful_emoji: ini.get_bool("task_bar", "enable_colorful_emoji", true),
        };
        taskbar.text_colors = std::mem::take(&mut text_colors);
        if taskbar.is_transparent(rt) {
            transparent_color_convert(&mut taskbar.back_color);
            transparent_color_convert(&mut taskbar.transparent_color);
        }
        transparent_color = taskbar.transparent_color;
        let _ = transparent_color;
        taskbar.valid();

        let cfg = ConfigData {
            auto_select: ini.get_bool("connection", "auto_select", true),
            select_all: ini.get_bool("connection", "select_all", false),
            connection_name: ini.get_string("connection", "connection_name", ""),
            use_log_scale: ini.get_bool("histroy_traffic", "use_log_scale", true),
            sunday_first: ini.get_bool("histroy_traffic", "sunday_first", true),
            view_type: HistoryViewType::from_i32(ini.get_int("histroy_traffic", "view_type", 0)),
            plugin_disabled: ini
                .get_string("config", "plugin_disabled", "")
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect(),
        };

        let styles = load_styles(&ini, &all, rt);

        let other = OtherSettings {
            notify_interval: ini.get_int("other", "notify_interval", 60),
            debug_log: ini.get_bool("other", "debug_log", false),
            last_light_mode: ini.get_bool("other", "last_light_mode", light),
        };

        Settings {
            taskbar,
            general,
            cfg,
            styles,
            other,
            config_dir,
        }
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join("config.ini")
    }

    pub fn history_path(&self) -> PathBuf {
        self.config_dir.join("history_traffic.dat")
    }

    pub fn save(&self, plugin_ids: &[String]) -> bool {
        let mut ini = Ini::load(&self.config_path());
        let all = all_items(plugin_ids);
        let g = &self.general;
        ini.set_bool("general", "check_update_when_start", g.check_update_when_start);
        ini.set_bool("general", "always_run_as_admin", g.always_run_as_admin);
        ini.set_string("general", "language", &g.language);
        ini.set_int("general", "update_source", g.update_source);
        ini.set_bool("general", "show_all_interface", g.show_all_interface);
        ini.set_int("general", "cpu_usage_acquire_method", g.cpu_usage_acquire_method);
        ini.set_int("general", "monitor_time_span", g.monitor_time_span);
        ini.set_string("general", "hard_disk_name", &g.hard_disk_name);
        ini.set_string("general", "cpu_core_name", &g.cpu_core_name);
        ini.set_u32("general", "hardware_monitor_item", g.hardware_monitor_item);
        ini.set_string_list("general", "connections_hide", &g.connections_hide);
        ini.set_bool("general", "auto_run_by_task_scheduler", g.auto_run_by_task_scheduler);

        ini.set_bool("connection", "auto_select", self.cfg.auto_select);
        ini.set_bool("connection", "select_all", self.cfg.select_all);
        ini.set_string("connection", "connection_name", &self.cfg.connection_name);

        ini.set_bool("notify_tip", "traffic_tip_enable", g.traffic_tip_enable);
        ini.set_int("notify_tip", "traffic_tip_value", g.traffic_tip_value);
        ini.set_int("notify_tip", "traffic_tip_unit", g.traffic_tip_unit);
        ini.set_bool("notify_tip", "memory_usage_tip_enable", g.memory_usage_tip.enable);
        ini.set_int("notify_tip", "memory_tip_value", g.memory_usage_tip.tip_value);
        ini.set_bool("notify_tip", "cpu_temperature_tip_enable", g.cpu_temp_tip.enable);
        ini.set_int("notify_tip", "cpu_temperature_tip_value", g.cpu_temp_tip.tip_value);
        ini.set_bool("notify_tip", "gpu_temperature_tip_enable", g.gpu_temp_tip.enable);
        ini.set_int("notify_tip", "gpu_temperature_tip_value", g.gpu_temp_tip.tip_value);
        ini.set_bool("notify_tip", "hdd_temperature_tip_enable", g.hdd_temp_tip.enable);
        ini.set_int("notify_tip", "hdd_temperature_tip_value", g.hdd_temp_tip.tip_value);
        ini.set_bool("notify_tip", "mainboard_temperature_tip_enable", g.mainboard_temp_tip.enable);
        ini.set_int("notify_tip", "mainboard_temperature_tip_value", g.mainboard_temp_tip.tip_value);

        let t = &self.taskbar;
        ini.set_u32("task_bar", "task_bar_back_color", t.back_color);
        ini.set_u32("task_bar", "transparent_color", t.transparent_color);
        ini.set_u32("task_bar", "status_bar_color", t.status_bar_color);
        save_colors(&mut ini, "task_bar", "task_bar_text_color", &all, &t.text_colors);
        ini.set_bool("task_bar", "specify_each_item_color", t.specify_each_item_color);
        ini.set_int("task_bar", "tbar_display_item", t.display_item.to_int());
        ini.set_string("task_bar", "font_name", &t.font.name);
        ini.set_int("task_bar", "font_size", t.font.size);
        let bits = (t.font.bold as i32) | ((t.font.italic as i32) << 1) | ((t.font.underline as i32) << 2) | ((t.font.strike_out as i32) << 3);
        ini.set_int("task_bar", "font_style", bits);
        ini.set_bool("task_bar", "show_taskbar_wnd_in_secondary_display", t.show_taskbar_wnd_in_secondary_display);
        ini.set_int("task_bar", "secondary_display_index", t.secondary_display_index);
        for d in ALL_BUILTIN {
            ini.set_string("task_bar", d.ini_key(), &t.label(&ItemKey::Builtin(d)));
        }
        for id in plugin_ids {
            if let Some(s) = t.disp_str.get(&ItemKey::Plugin(id.clone())) {
                ini.set_string("plugin_display_str_taskbar_window", id, s);
            }
        }
        ini.set_bool("task_bar", "task_bar_wnd_on_left", t.tbar_wnd_on_left);
        ini.set_bool("task_bar", "task_bar_wnd_snap", t.tbar_wnd_snap);
        ini.set_bool("task_bar", "task_bar_speed_short_mode", t.speed_short_mode);
        ini.set_bool("task_bar", "unit_byte", t.unit_byte);
        ini.set_int("task_bar", "task_bar_speed_unit", t.speed_unit as i32);
        ini.set_bool("task_bar", "task_bar_hide_unit", t.hide_unit);
        ini.set_bool("task_bar", "task_bar_hide_percent", t.hide_percent);
        ini.set_bool("task_bar", "value_right_align", t.value_right_align);
        ini.set_bool("task_bar", "horizontal_arrange", t.horizontal_arrange);
        ini.set_bool("task_bar", "show_status_bar", t.show_status_bar);
        ini.set_bool("task_bar", "separate_value_unit_with_space", t.separate_value_unit_with_space);
        ini.set_bool("task_bar", "show_tool_tip", t.show_tool_tip);
        ini.set_int("task_bar", "digits_number", t.digits_number);
        ini.set_int("task_bar", "memory_display", t.memory_display as i32);
        ini.set_int("task_bar", "double_click_action", t.double_click_action as i32);
        ini.set_string("task_bar", "double_click_exe", &t.double_click_exe);
        ini.set_bool("task_bar", "cm_graph_type", t.cm_graph_type);
        ini.set_bool("task_bar", "show_graph_dashed_box", t.show_graph_dashed_box);
        ini.set_int("task_bar", "item_space", t.item_space);
        ini.set_int("task_bar", "vertical_margin", t.vertical_margin);
        ini.set_int("task_bar", "window_offset_top", t.window_offset_top);
        ini.set_int("task_bar", "window_offset_left", t.window_offset_left);
        ini.set_bool("task_bar", "avoid_overlap_with_widgets", t.avoid_overlap_with_widgets);
        ini.set_int("task_bar", "taskbar_left_space_win11", t.taskbar_left_space_win11);
        ini.set_int("task_bar", "taskbar_right_space_win11", t.taskbar_right_space_win11);
        ini.set_bool("task_bar", "auto_adapt_light_theme", t.auto_adapt_light_theme);
        ini.set_int("task_bar", "dark_default_style", t.dark_default_style);
        ini.set_int("task_bar", "light_default_style", t.light_default_style);
        ini.set_string("task_bar", "item_order", &t.item_order.to_string());
        ini.set_string("task_bar", "plugin_display_item", &t.plugin_display_item.join(","));
        ini.set_bool("task_bar", "auto_save_taskbar_color_settings_to_preset", t.auto_save_taskbar_color_settings_to_preset);
        ini.set_bool("task_bar", "show_netspeed_figure", t.show_netspeed_figure);
        ini.set_int("task_bar", "netspeed_figure_max_value", t.netspeed_figure_max_value);
        ini.set_int("task_bar", "netspeed_figure_max_value_unit", t.netspeed_figure_max_value_unit);
        ini.set_bool("task_bar", "graph_color_following_system", t.graph_color_following_system);
        ini.set_bool("task_bar", "enable_colorful_emoji", t.enable_colorful_emoji);

        ini.set_bool("histroy_traffic", "use_log_scale", self.cfg.use_log_scale);
        ini.set_bool("histroy_traffic", "sunday_first", self.cfg.sunday_first);
        ini.set_int("histroy_traffic", "view_type", self.cfg.view_type as i32);

        ini.set_int("other", "notify_interval", self.other.notify_interval);
        ini.set_bool("other", "debug_log", self.other.debug_log);
        ini.set_bool("other", "last_light_mode", self.other.last_light_mode);

        ini.set_string("config", "plugin_disabled", &self.cfg.plugin_disabled.join(","));
        ini.set_string("app", "version", env!("CARGO_PKG_VERSION"));

        for (i, s) in self.styles.iter().enumerate() {
            if !s.is_valid() {
                continue;
            }
            let k = format!("default{}_", i + 1);
            save_colors(&mut ini, "taskbar_default_style", &(k.clone() + "text_color"), &all, &s.text_colors);
            ini.set_u32("taskbar_default_style", &(k.clone() + "back_color"), s.back_color);
            ini.set_u32("taskbar_default_style", &(k.clone() + "transparent_color"), s.transparent_color);
            ini.set_u32("taskbar_default_style", &(k.clone() + "status_bar_color"), s.status_bar_color);
            ini.set_bool("taskbar_default_style", &(k + "specify_each_item_color"), s.specify_each_item_color);
        }

        ini.save().is_ok()
    }

    /// `CTaskbarDefaultStyle::ApplyDefaultStyle`.
    pub fn apply_default_style(&mut self, index: i32) {
        let Some(style) = self.styles.get(index.max(0) as usize).filter(|_| index >= 0).cloned() else {
            return;
        };
        if !style.is_valid() {
            return;
        }
        let t = &mut self.taskbar;
        for (k, v) in &style.text_colors {
            t.text_colors.insert(k.clone(), *v);
        }
        t.back_color = style.back_color;
        t.transparent_color = style.transparent_color;
        t.status_bar_color = style.status_bar_color;
        t.specify_each_item_color = style.specify_each_item_color;
        if t.transparent_color == t.back_color {
            transparent_color_convert(&mut t.back_color);
            transparent_color_convert(&mut t.transparent_color);
        }
    }

    /// `CTaskbarDefaultStyle::ModifyDefaultStyle`.
    pub fn modify_default_style(&mut self, index: i32) {
        if let Some(style) = self.styles.get_mut(index.max(0) as usize).filter(|_| index >= 0) {
            style.text_colors = self.taskbar.text_colors.clone();
            style.back_color = self.taskbar.back_color;
            style.transparent_color = self.taskbar.transparent_color;
            style.status_bar_color = self.taskbar.status_bar_color;
            style.specify_each_item_color = self.taskbar.specify_each_item_color;
        }
    }
}

fn load_styles(ini: &Ini, all: &[ItemKey], rt: &Runtime) -> Vec<TaskbarStyle> {
    let mut styles = Vec::new();
    for i in 0..TASKBAR_DEFAULT_STYLE_NUM {
        let light_style = i == 2 || i == 3;
        let default_text_color = if light_style { rgb(0, 0, 0) } else { rgb(255, 255, 255) };
        let dark_back = if rt.version.is_windows8_or_81() { rgb(0, 0, 1) } else { 0 };
        let default_back = if light_style { rgb(210, 210, 211) } else { dark_back };
        let default_status = if light_style { rgb(165, 165, 165) } else { 0x005A5A5A };
        let k = format!("default{}_", i + 1);
        let (colors_default, specify_default) = match i {
            1 => (
                "4574711,16777215,5107653,16777215,16632499,16777215,16494036,16777215,14601983,16777215,16760992,10221779,13425662,16777215,4574711,16777215,".to_string(),
                true,
            ),
            2 => ("31957,0,2533120,0,13857281,0,14354551,0,1145775,0,4328380,0,167504,0,31957,0,".to_string(), true),
            _ => (default_text_color.to_string(), false),
        };
        let mut text_colors = load_colors(ini, "taskbar_default_style", &(k.clone() + "text_color"), all, &colors_default);
        if i == 1 || i == 2 {
            // Entries past the preset string fall back to the plain default text color.
            let parts = colors_default.split(',').filter(|s| !s.is_empty()).count();
            let stored = ini.contains("taskbar_default_style", &(k.clone() + "text_color"));
            if !stored {
                for (idx, item) in all.iter().enumerate() {
                    if idx * 2 >= parts {
                        text_colors.insert(
                            item.clone(),
                            ItemColor {
                                label: default_text_color,
                                value: default_text_color,
                            },
                        );
                    }
                }
            }
        }
        styles.push(TaskbarStyle {
            text_colors,
            back_color: ini.get_u32("taskbar_default_style", &(k.clone() + "back_color"), default_back),
            transparent_color: ini.get_u32("taskbar_default_style", &(k.clone() + "transparent_color"), default_back),
            status_bar_color: ini.get_u32("taskbar_default_style", &(k.clone() + "status_bar_color"), default_status),
            specify_each_item_color: ini.get_bool("taskbar_default_style", &(k + "specify_each_item_color"), specify_default),
        });
    }
    styles
}

pub fn rgb_to_hsl(c: u32) -> (f64, f64, f64) {
    let r = color_r(c) as f64 / 255.0;
    let g = color_g(c) as f64 / 255.0;
    let b = color_b(c) as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> u32 {
    if s == 0.0 {
        let v = (l * 255.0).round() as u8;
        return rgb(v, v, v);
    }
    fn hue(p: f64, q: f64, mut t: f64) -> f64 {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let r = hue(p, q, h + 1.0 / 3.0);
    let g = hue(p, q, h);
    let b = hue(p, q, h - 1.0 / 3.0);
    rgb((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8)
}
