//! Traffic history (`CHistoryTrafficDlg`): list view and calendar view.

use chrono::{Datelike, NaiveDate};
use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div,
    prelude::FluentBuilder, px, size, uniform_list,
};
use gpui_component::button::Button;
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{ActiveTheme, Sizable, h_flex, v_flex};

use super::Slot;
use super::common::{button, choice, label, row};
use crate::config::{HistoryViewType, rgb as colorref};
use crate::format::kbytes_to_string;
use crate::i18n::tr;
use crate::monitor::history::HistoryRecord;
use crate::state;

const TRAFFIC_COLOR_BLUE: u32 = colorref(0, 183, 238);
const TRAFFIC_COLOR_GREEN: u32 = colorref(128, 194, 105);
const TRAFFIC_COLOR_YELLOW: u32 = colorref(255, 216, 58);
const TRAFFIC_COLOR_RED: u32 = colorref(255, 95, 74);
const TRAFFIC_COLOR_DARK_RED: u32 = colorref(166, 19, 0);

fn traffic_color(kb: u64) -> u32 {
    if kb < 1024 * 1024 {
        TRAFFIC_COLOR_BLUE
    } else if kb < 10 * 1024 * 1024 {
        TRAFFIC_COLOR_GREEN
    } else if kb < 100 * 1024 * 1024 {
        TRAFFIC_COLOR_YELLOW
    } else if kb < 1024 * 1024 * 1024 {
        TRAFFIC_COLOR_RED
    } else {
        TRAFFIC_COLOR_DARK_RED
    }
}

fn hsla(c: u32) -> gpui::Hsla {
    super::common::colorref_to_hsla(c)
}

#[derive(Clone)]
struct Row {
    text: String,
    up: u64,
    down: u64,
    mixed: bool,
}

fn week_day_name(wd: u32) -> String {
    tr(match wd {
        0 => "IDS_SUNDAY",
        1 => "IDS_MONDAY",
        2 => "IDS_TUESDAY",
        3 => "IDS_WEDNESDAY",
        4 => "IDS_THURSDAY",
        5 => "IDS_FRIDAY",
        _ => "IDS_SATURDAY",
    })
}

/// `Date::week()`.
fn week_number(d: NaiveDate, sunday_first: bool) -> u32 {
    let jan1 = NaiveDate::from_ymd_opt(d.year(), 1, 1).unwrap_or(d);
    let wd = jan1.weekday().num_days_from_sunday() as i32;
    let days = d.ordinal() as i32 + if sunday_first { wd - 1 } else { wd - 2 };
    (days / 7 + 1).max(1) as u32
}

fn build_rows(records: &[HistoryRecord], view: HistoryViewType, sunday_first: bool) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for r in records {
        let text = match view {
            HistoryViewType::Day => {
                let wd = r
                    .date()
                    .map(|d| d.weekday().num_days_from_sunday())
                    .unwrap_or(0);
                format!(
                    "{:04}/{:02}/{:02} ({})",
                    r.year,
                    r.month,
                    r.day,
                    week_day_name(wd)
                )
            }
            HistoryViewType::Week => {
                let w = r.date().map(|d| week_number(d, sunday_first)).unwrap_or(1);
                format!(
                    "{:04}/{}",
                    r.year,
                    tr("IDS_WEEK_NUM").replace("<%1%>", &w.to_string())
                )
            }
            HistoryViewType::Month => format!("{:04}/{:02}", r.year, r.month),
            HistoryViewType::Quarter => format!("{:04}/Q{}", r.year, (r.month - 1) / 3 + 1),
            HistoryViewType::Year => format!("{:04}", r.year),
        };
        if view != HistoryViewType::Day {
            if let Some(last) = rows.last_mut().filter(|l| l.text == text) {
                last.up += r.up_kbytes;
                last.down += r.down_kbytes;
                last.mixed = last.mixed || r.mixed;
                continue;
            }
        }
        rows.push(Row {
            text,
            up: r.up_kbytes,
            down: r.down_kbytes,
            mixed: r.mixed,
        });
    }
    rows
}

pub struct HistoryView {
    tab: usize,
    records: Vec<HistoryRecord>,
    year: i32,
    month: u32,
}

impl HistoryView {
    fn new() -> Self {
        let records = state::with(|s| {
            s.monitor
                .as_ref()
                .map(|m| m.history.lock().unwrap().all())
                .unwrap_or_default()
        });
        let today = chrono::Local::now();
        HistoryView {
            tab: 0,
            records,
            year: today.year(),
            month: today.month(),
        }
    }

    fn set_cfg(&self, cx: &mut App, f: impl FnOnce(&mut crate::config::ConfigData)) {
        let _ = cx;
        state::with_mut(|s| {
            f(&mut s.settings.cfg);
            s.save_settings();
        });
    }

    fn render_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (view_type, log_scale, sunday_first) = state::with(|s| {
            (
                s.settings.cfg.view_type,
                s.settings.cfg.use_log_scale,
                s.settings.cfg.sunday_first,
            )
        });
        let rows = build_rows(&self.records, view_type, sunday_first);
        let max = rows.iter().map(|r| r.up + r.down).max().unwrap_or(1).max(1);
        let views = vec![
            tr("IDS_DAY_VIEW"),
            tr("IDS_WEEK_VIEW"),
            tr("IDS_MONTH_VIEW"),
            tr("IDS_QUARTER_VIEW"),
            tr("IDS_YEAR_VIEW"),
        ];
        let scales = vec![tr("IDS_LINEAR_SCALE"), tr("IDS_LOG_SCALE")];
        let me = cx.entity().downgrade();
        let me2 = me.clone();
        let header_color = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let rows_for_list = rows.clone();
        v_flex()
            .size_full()
            .gap_2()
            .child(
                row()
                    .child(label(tr("TXT_VIEW_TYPE")))
                    .child(choice(
                        "view-type",
                        views,
                        view_type as usize,
                        false,
                        move |i, _, cx| {
                            if let Some(v) = me.upgrade() {
                                v.update(cx, |v, cx| {
                                    v.set_cfg(cx, |c| {
                                        c.view_type = HistoryViewType::from_i32(i as i32)
                                    });
                                    cx.notify();
                                });
                            }
                        },
                    ))
                    .child(label(tr("TXT_FIGURE_VIEW_SCALE")))
                    .child(choice(
                        "view-scale",
                        scales,
                        log_scale as usize,
                        false,
                        move |i, _, cx| {
                            if let Some(v) = me2.upgrade() {
                                v.update(cx, |v, cx| {
                                    v.set_cfg(cx, |c| c.use_log_scale = i == 1);
                                    cx.notify();
                                });
                            }
                        },
                    )),
            )
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(border)
                    .text_sm()
                    .text_color(header_color)
                    .child(div().w(px(200.)).child(tr("IDS_DATE")))
                    .child(div().w(px(100.)).child(tr("IDS_UPLOAD")))
                    .child(div().w(px(100.)).child(tr("IDS_DOWNLOAD")))
                    .child(div().w(px(100.)).child(tr("IDS_TRAFFIC_USED")))
                    .child(div().flex_1().child(tr("IDS_FIGURE"))),
            )
            .child(
                uniform_list("history-rows", rows.len(), move |range, _window, cx| {
                    let odd_bg = cx.theme().muted.opacity(0.35);
                    range
                        .map(|ix| {
                            let r = &rows_for_list[ix];
                            let total = r.up + r.down;
                            let mut ratio = total as f64 * 1000.0 / max as f64;
                            if log_scale {
                                ratio = (ratio + 1.0).ln() / (1001f64).ln();
                            } else {
                                ratio /= 1000.0;
                            }
                            let ratio = ratio.clamp(0.0, 1.0) as f32;
                            h_flex()
                                .id(ix)
                                .w_full()
                                .h(px(24.))
                                .px_2()
                                .text_sm()
                                .when(ix % 2 == 1, |d| d.bg(odd_bg))
                                .child(div().w(px(200.)).child(r.text.clone()))
                                .child(div().w(px(100.)).child(if r.mixed {
                                    "-".into()
                                } else {
                                    kbytes_to_string(r.up)
                                }))
                                .child(div().w(px(100.)).child(if r.mixed {
                                    "-".into()
                                } else {
                                    kbytes_to_string(r.down)
                                }))
                                .child(div().w(px(100.)).child(kbytes_to_string(total)))
                                .child(
                                    div().flex_1().h(px(12.)).child(
                                        div()
                                            .h_full()
                                            .w(gpui::relative(ratio))
                                            .bg(hsla(traffic_color(total))),
                                    ),
                                )
                        })
                        .collect::<Vec<_>>()
                })
                .flex_1(),
            )
    }

    fn render_calendar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let sunday_first = state::with(|s| s.settings.cfg.sunday_first);
        let (year, month) = (self.year, self.month);
        let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_default();
        let days_in_month = {
            let next = if month == 12 {
                NaiveDate::from_ymd_opt(year + 1, 1, 1)
            } else {
                NaiveDate::from_ymd_opt(year, month + 1, 1)
            };
            next.map(|n| (n - first).num_days() as u32).unwrap_or(30)
        };
        let offset = if sunday_first {
            first.weekday().num_days_from_sunday()
        } else {
            first.weekday().num_days_from_monday()
        };
        let traffic_of = |day: u32| -> Option<&HistoryRecord> {
            self.records
                .iter()
                .find(|r| r.year == year && r.month == month && r.day == day)
        };
        let month_total: u64 = self
            .records
            .iter()
            .filter(|r| r.year == year && r.month == month)
            .map(|r| r.kbytes())
            .sum();
        let today = chrono::Local::now().date_naive();
        let years: Vec<i32> = {
            let mut ys: Vec<i32> = self.records.iter().map(|r| r.year).collect();
            ys.push(today.year());
            ys.sort();
            ys.dedup();
            ys
        };
        let year_names: Vec<String> = years.iter().map(|y| y.to_string()).collect();
        let year_index = years.iter().position(|y| *y == year).unwrap_or(0);
        let month_names: Vec<String> = (1..=12).map(|m| m.to_string()).collect();
        let order: Vec<u32> = if sunday_first {
            vec![0, 1, 2, 3, 4, 5, 6]
        } else {
            vec![1, 2, 3, 4, 5, 6, 0]
        };

        let mut cells = Vec::new();
        for _ in 0..offset {
            cells.push(div().h(px(52.)).into_any_element());
        }
        for day in 1..=days_in_month {
            let rec = traffic_of(day).copied();
            let is_today = today.year() == year && today.month() == month && today.day() == day;
            let kb = rec.map(|r| r.kbytes()).unwrap_or(0);
            cells.push(
                v_flex()
                    .h(px(52.))
                    .p_1()
                    .gap_0p5()
                    .border_1()
                    .border_color(if is_today {
                        cx.theme().primary
                    } else {
                        cx.theme().border
                    })
                    .rounded(px(4.))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(day.to_string()),
                    )
                    .when(kb > 0, |c| {
                        c.child(div().text_xs().child(kbytes_to_string(kb)))
                            .child(div().h(px(4.)).w_full().bg(hsla(traffic_color(kb))))
                    })
                    .into_any_element(),
            );
        }
        let me = cx.entity().downgrade();
        let (m1, m2, m3, m4, m5) = (me.clone(), me.clone(), me.clone(), me.clone(), me.clone());
        let years_for_select = years.clone();
        let legend = [
            (TRAFFIC_COLOR_BLUE, "< 1GB"),
            (TRAFFIC_COLOR_GREEN, "1GB~10GB"),
            (TRAFFIC_COLOR_YELLOW, "10GB~100GB"),
            (TRAFFIC_COLOR_RED, "100GB~1TB"),
            (TRAFFIC_COLOR_DARK_RED, "> 1TB"),
        ];
        let menu_me = me.clone();
        v_flex()
            .size_full()
            .gap_2()
            .child(
                row()
                    .child(label(tr("TXT_YEAR")))
                    .child(choice(
                        "cal-year",
                        year_names,
                        year_index,
                        false,
                        move |i, _, cx| {
                            let y = years_for_select[i];
                            if let Some(v) = m1.upgrade() {
                                v.update(cx, |v, cx| {
                                    v.year = y;
                                    cx.notify();
                                });
                            }
                        },
                    ))
                    .child(label(tr("TXT_MONTH")))
                    .child(choice(
                        "cal-month",
                        month_names,
                        (month - 1) as usize,
                        false,
                        move |i, _, cx| {
                            if let Some(v) = m2.upgrade() {
                                v.update(cx, |v, cx| {
                                    v.month = i as u32 + 1;
                                    cx.notify();
                                });
                            }
                        },
                    ))
                    .child(button("cal-prev", "<", false, move |_, cx| {
                        if let Some(v) = m3.upgrade() {
                            v.update(cx, |v, cx| {
                                if v.month == 1 {
                                    v.month = 12;
                                    v.year -= 1;
                                } else {
                                    v.month -= 1;
                                }
                                cx.notify();
                            });
                        }
                    }))
                    .child(button("cal-next", ">", false, move |_, cx| {
                        if let Some(v) = m4.upgrade() {
                            v.update(cx, |v, cx| {
                                if v.month == 12 {
                                    v.month = 1;
                                    v.year += 1;
                                } else {
                                    v.month += 1;
                                }
                                cx.notify();
                            });
                        }
                    }))
                    .child(
                        Button::new("cal-menu")
                            .label("≡")
                            .small()
                            .outline()
                            .dropdown_menu(move |menu, _, _| {
                                let (a, b, c) = (menu_me.clone(), menu_me.clone(), m5.clone());
                                menu.label(tr("TXT_FIRST_DAT_OF_WEEEK"))
                                    .item(
                                        PopupMenuItem::new(tr("TXT_SUNDAY"))
                                            .checked(sunday_first)
                                            .on_click(move |_, _, cx| {
                                                if let Some(v) = a.upgrade() {
                                                    v.update(cx, |v, cx| {
                                                        v.set_cfg(cx, |c| c.sunday_first = true);
                                                        cx.notify();
                                                    });
                                                }
                                            }),
                                    )
                                    .item(
                                        PopupMenuItem::new(tr("TXT_MONDAY"))
                                            .checked(!sunday_first)
                                            .on_click(move |_, _, cx| {
                                                if let Some(v) = b.upgrade() {
                                                    v.update(cx, |v, cx| {
                                                        v.set_cfg(cx, |c| c.sunday_first = false);
                                                        cx.notify();
                                                    });
                                                }
                                            }),
                                    )
                                    .separator()
                                    .item(
                                        PopupMenuItem::new(crate::i18n::strip_mnemonic(&tr(
                                            "TXT_GO_TO_TODAY",
                                        )))
                                        .on_click(
                                            move |_, _, cx| {
                                                if let Some(v) = c.upgrade() {
                                                    v.update(cx, |v, cx| {
                                                        let t = chrono::Local::now();
                                                        v.year = t.year();
                                                        v.month = t.month();
                                                        cx.notify();
                                                    });
                                                }
                                            },
                                        ),
                                    )
                            }),
                    ),
            )
            .child(h_flex().children(order.iter().map(|wd| {
                div()
                    .flex_1()
                    .text_center()
                    .text_sm()
                    .text_color(if *wd == 0 || *wd == 6 {
                        cx.theme().danger
                    } else {
                        cx.theme().foreground
                    })
                    .child(week_day_name(*wd))
            })))
            .child(div().grid().grid_cols(7).gap_1().children(cells))
            .child(row().child(label(format!(
                "{} {}",
                tr("IDS_CURRENT_MONTH_TOTAL_TRAFFIC"),
                kbytes_to_string(month_total)
            ))))
            .child(
                row()
                    .child(label(tr("IDS_LEGEND")))
                    .children(legend.iter().map(|(c, t)| {
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(div().size(px(12.)).bg(hsla(*c)))
                            .child(div().text_xs().child(*t))
                    })),
            )
    }
}

impl Render for HistoryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let me = cx.entity().downgrade();
        let content = if self.tab == 0 {
            self.render_list(cx).into_any_element()
        } else {
            self.render_calendar(cx).into_any_element()
        };
        v_flex()
            .size_full()
            .p_3()
            .gap_2()
            .child(
                TabBar::new("history-tabs")
                    .underline()
                    .selected_index(self.tab)
                    .on_click(move |ix, _, cx| {
                        if let Some(v) = me.upgrade() {
                            v.update(cx, |v, cx| {
                                v.tab = *ix;
                                cx.notify();
                            });
                        }
                    })
                    .child(Tab::new().label(tr("IDS_LIST_VIEW")))
                    .child(Tab::new().label(tr("IDS_CALENDAR_VIEW"))),
            )
            .child(div().flex_1().min_h_0().child(content))
    }
}

pub fn open(cx: &mut App) {
    super::open_window(
        Slot::History,
        tr("TXT_TITLE_HISTORY_TRAFFIC"),
        size(px(720.), px(620.)),
        true,
        cx,
        |_, _| HistoryView::new(),
    );
}
