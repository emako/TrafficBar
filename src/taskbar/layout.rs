//! Item measurement, layout and drawing (`CTaskBarDlg::CalculateWindowSize`, `DrawDisplayItem`,
//! `DrawPluginItem`, `TryDrawStatusBar`, `TryDrawGraph`).

use std::collections::{HashMap, VecDeque};

use super::render::{Align, Devices, Font, Frame, Rect};
use crate::config::{Runtime, TaskbarSettings};
use crate::display_item::{all_items, DisplayItem, ItemKey};
use crate::format;
use crate::monitor::Snapshot;
use crate::plugin::PluginHost;

const TASKBAR_GRAPH_STEP: u32 = 5;

#[derive(Clone, Debug)]
pub struct ItemLayout {
    pub key: ItemKey,
    pub label_width: i32,
    pub value_width: i32,
    pub double_line_exclusive: bool,
    pub is_double_line: bool,
    pub rect: Rect,
}

impl ItemLayout {
    fn total(&self) -> i32 {
        self.label_width + self.value_width
    }
    fn max(&self) -> i32 {
        self.label_width.max(self.value_width)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub items: Vec<ItemLayout>,
    pub width: i32,
    pub height: i32,
}

impl Layout {
    pub fn hit(&self, x: i32, y: i32) -> Option<&ItemLayout> {
        self.items.iter().find(|i| i.rect.contains(x, y))
    }
}

pub struct Context<'a> {
    pub settings: &'a TaskbarSettings,
    pub snapshot: &'a Snapshot,
    pub plugins: &'a PluginHost,
    pub rt: &'a Runtime,
    pub dpi: u32,
    pub on_top_or_bottom: bool,
    pub taskbar_width: i32,
}

impl Context<'_> {
    fn dpi(&self, px: i32) -> i32 {
        px * self.dpi as i32 / 96
    }

    pub fn wnd_height(&self) -> i32 {
        self.dpi(32)
    }

    /// Temperatures need LibreHardwareMonitor, which only runs elevated.
    fn admin_blocked(&self, item: DisplayItem) -> bool {
        item.requires_admin() && !self.rt.elevated
    }

    pub fn value_text(&self, key: &ItemKey) -> String {
        match key {
            ItemKey::Builtin(d) => {
                if self.admin_blocked(*d) {
                    "!".into()
                } else {
                    format::item_value_text(*d, self.snapshot, self.settings)
                }
            }
            ItemKey::Plugin(id) => match self.plugins.item(id) {
                Some(item) => {
                    let v = self.plugins.value_text(item);
                    if v.is_empty() && self.settings.label(key).is_empty() {
                        item.name.clone()
                    } else {
                        v
                    }
                }
                None => String::new(),
            },
        }
    }

    fn sample_text(&self, key: &ItemKey) -> String {
        match key {
            ItemKey::Builtin(d) => format::item_sample_text(*d, self.snapshot, self.settings),
            ItemKey::Plugin(id) => match self.plugins.item(id) {
                Some(item) => {
                    let s = self.plugins.sample_text(item);
                    if s.is_empty() && self.settings.label(key).is_empty() {
                        item.name.clone()
                    } else {
                        s
                    }
                }
                None => String::new(),
            },
        }
    }

    fn is_displayed(&self, key: &ItemKey) -> bool {
        match key {
            ItemKey::Builtin(_) => self.settings.is_item_displayed(key),
            ItemKey::Plugin(id) => self.settings.is_item_displayed(key) && self.plugins.item(id).is_some(),
        }
    }
}

pub fn calculate(dev: &Devices, font: &Font, ctx: &Context) -> Layout {
    let s = ctx.settings;
    let horizontal_arrange = s.horizontal_arrange && ctx.on_top_or_bottom;
    let plugin_ids = ctx.plugins.item_ids();
    let all = all_items(&plugin_ids);
    let ordered = s.item_order.ordered(&all);

    let mut items: Vec<ItemLayout> = ordered
        .into_iter()
        .filter(|k| ctx.is_displayed(k))
        .map(|key| {
            let label_width = font.text_width(dev, &s.label(&key));
            let value_width = font.text_width(dev, &ctx.sample_text(&key));
            let double_line_exclusive = match &key {
                ItemKey::Plugin(id) => ctx.plugins.item(id).map(|i| ctx.plugins.is_double_line(i)).unwrap_or(false),
                _ => false,
            };
            ItemLayout {
                key,
                label_width,
                value_width,
                double_line_exclusive,
                is_double_line: false,
                rect: Rect::default(),
            }
        })
        .collect();
    if items.is_empty() {
        let key = ItemKey::Builtin(DisplayItem::Up);
        items.push(ItemLayout {
            label_width: font.text_width(dev, &s.label(&key)),
            value_width: font.text_width(dev, &ctx.sample_text(&key)),
            key,
            double_line_exclusive: false,
            is_double_line: false,
            rect: Rect::default(),
        });
    }

    let item_space = ctx.dpi(s.item_space);
    let wnd_h = ctx.wnd_height();
    let mut width;
    let height;

    if ctx.on_top_or_bottom {
        height = if !horizontal_arrange { wnd_h } else { wnd_h * 2 / 3 };
        if s.horizontal_arrange {
            width = 0;
            let mut x = 0;
            for (i, it) in items.iter_mut().enumerate() {
                width += it.total();
                if i > 0 {
                    x += item_space;
                }
                it.rect = Rect::new(x, 0, x + it.total(), height);
                x = it.rect.right;
            }
            width += item_space * items.len() as i32;
        } else {
            let mut current_x = item_space;
            let item_height = height / 2;
            let y_up = -ctx.dpi(s.vertical_margin) / 2;
            let y_down = y_up + item_height + ctx.dpi(s.vertical_margin);
            let mut first: Option<usize> = None;
            let mut width0 = 0;
            let mut max_width0 = 0;
            for i in 0..items.len() {
                if items[i].double_line_exclusive {
                    if let Some(f) = first.take() {
                        items[f].rect = Rect::new(current_x, 0, current_x + max_width0, height);
                        items[f].is_double_line = true;
                        current_x += max_width0 + item_space;
                    }
                    let col = items[i].max();
                    items[i].rect = Rect::new(current_x, 0, current_x + col, height);
                    items[i].is_double_line = true;
                    current_x += col + item_space;
                } else {
                    items[i].is_double_line = false;
                    match first {
                        None => {
                            width0 = items[i].total();
                            max_width0 = items[i].max();
                            items[i].rect = Rect::new(current_x, y_up, current_x + width0, y_up + item_height);
                            first = Some(i);
                        }
                        Some(f) => {
                            let col = width0.max(items[i].total());
                            items[f].rect.right = current_x + col;
                            items[i].rect = Rect::new(current_x, y_down, current_x + col, y_down + item_height);
                            current_x += col + item_space;
                            first = None;
                        }
                    }
                }
            }
            if let Some(f) = first {
                items[f].rect = Rect::new(current_x, 0, current_x + max_width0, height);
                items[f].is_double_line = true;
                current_x += max_width0 + item_space;
            }
            width = current_x;
        }
    } else {
        width = items.iter().map(|i| i.total()).max().unwrap_or(0);
        let mut h = 0;
        for it in items.iter_mut() {
            let item_height = if it.double_line_exclusive { wnd_h } else { wnd_h / 2 };
            let item_width = width.min(ctx.taskbar_width - ctx.dpi(2));
            it.rect = Rect::new(0, h, item_width, h + item_height);
            h += item_height + item_space;
        }
        height = h;
    }

    Layout {
        items,
        width: width.max(1),
        height: height.max(1),
    }
}

/// Usage history used by the scrolling graph (`m_map_history_data`).
#[derive(Default)]
pub struct GraphHistory {
    data: HashMap<ItemKey, (VecDeque<i32>, u32)>,
}

impl GraphHistory {
    /// `CTaskBarDlg::AddHisToList`.
    fn add(&mut self, key: &ItemKey, value: i32, max_len: usize) {
        let (list, count) = self.data.entry(key.clone()).or_default();
        if *count % TASKBAR_GRAPH_STEP == 0 {
            if let Some(front) = list.front_mut() {
                *front /= TASKBAR_GRAPH_STEP as i32;
            }
            list.push_front(value);
        } else if let Some(front) = list.front_mut() {
            *front += value;
        } else {
            list.push_front(value);
        }
        while list.len() > max_len.max(1) {
            list.pop_back();
        }
        *count = count.wrapping_add(1);
    }

    fn get(&self, key: &ItemKey) -> Option<&VecDeque<i32>> {
        self.data.get(key).map(|(l, _)| l)
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }
}

fn netspeed_percent(speed: u64, s: &TaskbarSettings) -> i32 {
    let max = s.netspeed_figure_max_bytes();
    if speed >= max {
        100
    } else if max > 0 {
        (speed * 100 / max) as i32
    } else {
        0
    }
}

fn draw_status_bar(frame: &Frame, r: Rect, percent: i32, graph_color: u32, dashed_box: bool) {
    let p = percent.clamp(0, 100);
    let fill = Rect::new(r.left, r.top, r.left + r.width() * p / 100, r.bottom);
    if dashed_box {
        frame.outline(r, graph_color, true);
    }
    frame.fill_rect(fill, graph_color, 255);
}

fn draw_graph(frame: &Frame, r: Rect, list: Option<&VecDeque<i32>>, graph_color: u32, dashed_box: bool) {
    if dashed_box {
        frame.outline(r, graph_color, true);
    }
    let Some(list) = list else { return };
    for (i, v) in list.iter().enumerate() {
        if i == 0 {
            continue;
        }
        if i as i32 >= r.width() {
            break;
        }
        let v = (*v).clamp(0, 100);
        frame.vline(r.right - i as i32, r.bottom, v * r.height() / 100, graph_color);
    }
}

pub fn draw(frame: &Frame, layout: &Layout, ctx: &Context, graphs: &mut GraphHistory) {
    let s = ctx.settings;
    let graph_color = s.usage_graph_color(ctx.rt);
    for it in &layout.items {
        if it.rect.is_empty() {
            continue;
        }
        let rect = it.rect;
        let vertical = it.is_double_line;
        let mut rect_label = rect;
        let mut rect_value = rect;
        if !vertical {
            rect_label.right = rect_label.left + it.label_width;
            rect_value.left = rect_label.right;
        } else if it.label_width != 0 {
            rect_label.bottom = rect_label.top + rect_label.height() / 2;
            rect_value.top = rect_label.bottom;
        }

        let colors = s.item_color(&it.key);
        let figure: Option<i32> = match &it.key {
            ItemKey::Builtin(d) => {
                let snap = ctx.snapshot;
                let v = match d {
                    DisplayItem::Cpu => Some(snap.cpu_usage),
                    DisplayItem::Memory => Some(snap.memory_usage),
                    DisplayItem::GpuUsage => Some(snap.gpu_usage),
                    DisplayItem::CpuTemp => Some(snap.cpu_temperature as i32),
                    DisplayItem::GpuTemp => Some(snap.gpu_temperature as i32),
                    DisplayItem::HddTemp => Some(snap.hdd_temperature as i32),
                    DisplayItem::MainBoardTemp => Some(snap.main_board_temperature as i32),
                    DisplayItem::HddUsage => Some(snap.hdd_usage),
                    DisplayItem::Up => Some(netspeed_percent(snap.out_speed, s)),
                    DisplayItem::Down => Some(netspeed_percent(snap.in_speed, s)),
                    DisplayItem::TotalSpeed => Some(netspeed_percent(snap.in_speed + snap.out_speed, s)),
                    DisplayItem::CpuFreq | DisplayItem::TodayTraffic => None,
                };
                let enabled = if d.is_net_speed() { s.show_netspeed_figure } else { s.show_status_bar };
                if enabled && !ctx.admin_blocked(*d) { v } else { None }
            }
            ItemKey::Plugin(id) => {
                if s.show_status_bar {
                    ctx.plugins
                        .item(id)
                        .and_then(|i| ctx.plugins.graph_value(i))
                        .map(|v| (v * 100.0) as i32)
                } else {
                    None
                }
            }
        };
        if let Some(value) = figure {
            if s.cm_graph_type {
                graphs.add(&it.key, value, rect.width().max(1) as usize);
                draw_graph(frame, rect, graphs.get(&it.key), graph_color, s.show_graph_dashed_box);
            } else {
                draw_status_bar(frame, rect, value, graph_color, s.show_graph_dashed_box);
            }
        }

        if let ItemKey::Plugin(id) = &it.key {
            ctx.plugins.notify_draw_colors(id, colors.label, colors.value);
        }

        if it.label_width > 0 {
            frame.text(
                rect_label,
                &s.label(&it.key),
                colors.label,
                if vertical { Align::Center } else { Align::Left },
            );
        }
        let align = if vertical {
            Align::Center
        } else if s.value_right_align {
            Align::Right
        } else {
            Align::Left
        };
        frame.text(rect_value, &ctx.value_text(&it.key), colors.value, align);
    }
}
