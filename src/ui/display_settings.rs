//! "显示设置" (`CSetItemOrderDlg`): which items the taskbar window shows and in what order.

use gpui::{
    AnyElement, App, Context, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement,
    Styled, Window, div, prelude::FluentBuilder, px, size,
};
use gpui_component::{ActiveTheme, h_flex, scroll::ScrollableElement, v_flex};

use super::common::{self, button, footer, primary_button};
use super::Slot;
use crate::config::TaskbarSettings;
use crate::display_item::{all_items, ItemKey, ItemOrder};
use crate::i18n::{tr, tr_plain};
use crate::state;

#[derive(Clone)]
pub struct OrderEntry {
    pub key: ItemKey,
    pub name: String,
    pub checked: bool,
    /// Index into `all_items`.
    pub index: i32,
}

#[derive(Clone)]
pub struct ItemOrderModel {
    pub entries: Vec<OrderEntry>,
    pub selected: Option<usize>,
}

impl ItemOrderModel {
    pub fn from_settings(t: &TaskbarSettings, hardware_item_enabled: impl Fn(&ItemKey) -> bool) -> Self {
        let (ids, names) = state::with(|s| {
            (
                s.plugins.item_ids(),
                s.plugins.items.iter().map(|i| (i.id.clone(), i.name.clone())).collect::<Vec<_>>(),
            )
        });
        let all = all_items(&ids);
        let mut entries = Vec::new();
        for i in &t.item_order.order {
            let Some(key) = all.get(*i as usize) else { continue };
            if !hardware_item_enabled(key) {
                continue;
            }
            let name = match key {
                ItemKey::Builtin(d) => d.name(),
                ItemKey::Plugin(id) => names.iter().find(|(pid, _)| pid == id).map(|n| n.1.clone()).unwrap_or_else(|| id.clone()),
            };
            entries.push(OrderEntry {
                key: key.clone(),
                name,
                checked: t.is_item_displayed(key),
                index: *i,
            });
        }
        ItemOrderModel { entries, selected: None }
    }

    pub fn move_up(&mut self) {
        if let Some(i) = self.selected.filter(|i| *i > 0) {
            self.entries.swap(i, i - 1);
            self.selected = Some(i - 1);
        }
    }

    pub fn move_down(&mut self) {
        if let Some(i) = self.selected.filter(|i| *i + 1 < self.entries.len()) {
            self.entries.swap(i, i + 1);
            self.selected = Some(i + 1);
        }
    }

    pub fn restore_default(&mut self) {
        self.entries.sort_by_key(|e| e.index);
    }

    pub fn toggle(&mut self, i: usize, on: bool) {
        if let Some(e) = self.entries.get_mut(i) {
            e.checked = on;
        }
    }

    pub fn apply_to(&self, t: &mut TaskbarSettings, all_count: usize) {
        let mut order: Vec<i32> = self.entries.iter().map(|e| e.index).collect();
        for i in &t.item_order.order {
            if !order.contains(i) {
                order.push(*i);
            }
        }
        t.item_order = ItemOrder { order };
        t.item_order.normalize(all_count);
        for e in &self.entries {
            t.set_item_displayed(&e.key, e.checked);
        }
        if t.display_item.is_empty() && t.plugin_display_item.is_empty() {
            t.display_item.set(crate::display_item::DisplayItem::Up, true);
        }
    }

    /// The check list plus the move/restore buttons. `update` mutates the model owned by the caller.
    pub fn render(
        &self,
        id_prefix: &str,
        update: impl Fn(&mut Window, &mut App, &dyn Fn(&mut ItemOrderModel)) + Clone + 'static,
        cx: &App,
    ) -> AnyElement {
        let elevated = crate::win::is_elevated();
        let list = v_flex()
            .id(gpui::ElementId::Name(format!("{id_prefix}-list").into()))
            .flex_1()
            .min_h(px(240.))
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .overflow_y_scrollbar()
            .children(self.entries.iter().enumerate().map(|(i, e)| {
                let up = update.clone();
                let up2 = update.clone();
                let selected = self.selected == Some(i);
                h_flex()
                    .id(gpui::ElementId::Name(format!("{id_prefix}-row-{i}").into()))
                    .px_2()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .when(selected, |d| d.bg(cx.theme().accent))
                    .on_click(move |_, window, cx| up(window, cx, &|m| m.selected = Some(i)))
                    .child(common::check(
                        format!("{id_prefix}-chk-{i}"),
                        e.name.clone(),
                        e.checked,
                        false,
                        false,
                        move |v, window, cx| up2(window, cx, &|m| m.toggle(i, v)),
                        cx,
                    ))
                    .when(
                        matches!(&e.key, ItemKey::Builtin(d) if d.requires_admin()) && !elevated,
                        |d| d.child(common::admin_mark(format!("{id_prefix}-adm-{i}"), cx)),
                    )
            }));
        let u1 = update.clone();
        let u2 = update.clone();
        let u3 = update.clone();
        h_flex()
            .gap_3()
            .items_start()
            .child(list)
            .child(
                v_flex()
                    .gap_2()
                    .w(px(110.))
                    .child(button(format!("{id_prefix}-up"), tr_plain("TXT_MOVE_UP_BUTTON"), self.selected.is_none_or(|i| i == 0), move |w, cx| u1(w, cx, &|m| m.move_up())))
                    .child(button(
                        format!("{id_prefix}-down"),
                        tr_plain("TXT_MOVE_DOWN_BUTTON"),
                        self.selected.is_none_or(|i| i + 1 >= self.entries.len()),
                        move |w, cx| u2(w, cx, &|m| m.move_down()),
                    ))
                    .child(button(format!("{id_prefix}-default"), tr_plain("TXT_RESTORE_DEFAULT_BUTTON"), false, move |w, cx| {
                        u3(w, cx, &|m| m.restore_default())
                    })),
            )
            .into_any_element()
    }
}

pub fn hardware_filter(general: &crate::config::GeneralSettings) -> impl Fn(&ItemKey) -> bool + '_ {
    move |k: &ItemKey| match k {
        ItemKey::Builtin(d) => d.hardware().map(|hw| general.is_hardware_enabled(hw)).unwrap_or(true),
        ItemKey::Plugin(_) => true,
    }
}

pub struct DisplaySettingsView {
    model: ItemOrderModel,
}

impl Render for DisplaySettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let update = move |_w: &mut Window, cx: &mut App, f: &dyn Fn(&mut ItemOrderModel)| {
            if let Some(v) = this.upgrade() {
                v.update(cx, |v, cx| {
                    f(&mut v.model);
                    cx.notify();
                });
            }
        };
        let ok_view = cx.entity().downgrade();
        v_flex()
            .size_full()
            .child(div().flex_1().p_4().child(self.model.render("ds", update, cx)))
            .child(footer(
                vec![
                    primary_button("ds-ok", tr("TXT_OK"), move |window, cx| {
                        if let Some(v) = ok_view.upgrade() {
                            let model = v.read(cx).model.clone();
                            let mut settings = state::with(|s| s.settings.clone());
                            let count = state::with(|s| s.plugins.item_ids().len()) + crate::display_item::ALL_BUILTIN.len();
                            model.apply_to(&mut settings.taskbar, count);
                            crate::app::apply_settings(settings);
                        }
                        window.remove_window();
                    }),
                    button("ds-cancel", tr("TXT_CANCEL"), false, |window, _| window.remove_window()),
                ],
                cx,
            ))
    }
}

pub fn open(cx: &mut App) {
    let model = state::with(|s| ItemOrderModel::from_settings(&s.settings.taskbar, hardware_filter(&s.settings.general)));
    super::open_window(
        Slot::DisplaySettings,
        tr("TXT_TITLE_SELECT_ORDER_DIALOG"),
        size(px(460.), px(480.)),
        true,
        cx,
        move |_, _| DisplaySettingsView { model },
    );
}