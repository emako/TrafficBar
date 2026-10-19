//! Connection details (`CNetworkInfoDlg`).

use gpui::{
    App, AppContext, Context, InteractiveElement, IntoElement, ParentElement, Render, SharedString, Styled, Window,
    div, prelude::FluentBuilder, px, size,
};
use gpui_component::{ActiveTheme, InteractiveElementExt, h_flex, scroll::ScrollableElement, v_flex};

use super::common::{button, footer, kv_row};
use super::Slot;
use crate::format::{data_size_to_string, int_with_separator};
use crate::i18n::tr;
use crate::monitor::net;
use crate::monitor::Selection;
use crate::state;

pub struct NetworkInfoView {
    index: usize,
    adapters: Vec<net::AdapterAddresses>,
    internet_v4: Option<String>,
    internet_v6: Option<String>,
}

fn if_type_text(t: u32) -> String {
    tr(match t {
        1 => "IDS_IF_TYPE_OTHER",
        6 => "IDS_IF_TYPE_ETHERNET_CSMACD",
        9 => "IDS_IF_TYPE_ISO88025_TOKENRING",
        15 => "IDS_IF_TYPE_FDDI",
        23 => "IDS_IF_TYPE_PPP",
        24 => "IDS_IF_TYPE_SOFTWARE_LOOPBACK",
        37 => "IDS_IF_TYPE_ATM",
        71 => "IDS_IF_TYPE_IEEE80211",
        131 => "IDS_IF_TYPE_TUNNEL",
        144 => "IDS_IF_TYPE_IEEE1394",
        237 => "IDS_IF_TYPE_IEEE80216_WMAN",
        243 => "IDS_IF_TYPE_WWANPP",
        244 => "IDS_IF_TYPE_WWANPP2",
        _ => "IDS_UNKNOW_CONNECTION",
    })
}

fn oper_status_text(s: i32) -> String {
    tr(match s {
        1 => "IDS_IF_OPER_STATUS_UP",
        2 | 6 | 7 => "IDS_IF_OPER_STATUS_DOWN",
        5 => "IDS_IF_OPER_STATUS_DORMANT",
        _ => "IDS_UNKNOW_STATUS",
    })
}

fn bytes_text(n: u64) -> String {
    format!("{} ({})", int_with_separator(n), data_size_to_string(n, true))
}

fn parse_ip_json(raw: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let ip = v.get("ip")?.as_str()?.to_string();
    match v.get("location").and_then(|l| l.as_str()).filter(|l| !l.is_empty()) {
        Some(loc) => Some(format!("{ip} ({loc})")),
        None => Some(ip),
    }
}

impl NetworkInfoView {
    fn new(cx: &mut Context<Self>) -> Self {
        let index = state::with(|s| s.selected_connection);
        let view = NetworkInfoView {
            index,
            adapters: net::adapters(),
            internet_v4: None,
            internet_v6: None,
        };
        let ua = format!("TrafficBar/{}", env!("CARGO_PKG_VERSION"));
        cx.spawn(async move |this, cx| {
            let ua4 = ua.clone();
            let v4 = cx
                .background_spawn(async move { crate::http::get("https://v4.yinghualuo.cn/bejson", &ua4).and_then(|r| parse_ip_json(&r)) })
                .await;
            let v6 = cx
                .background_spawn(async move { crate::http::get("https://v6.yinghualuo.cn/bejson", &ua).and_then(|r| parse_ip_json(&r)) })
                .await;
            let _ = this.update(cx, |v, cx| {
                v.internet_v4 = Some(v4.unwrap_or_else(|| tr("IDS_GET_FAILED")));
                v.internet_v6 = Some(v6.unwrap_or_else(|| tr("IDS_GET_FAILED")));
                cx.notify();
            });
        })
        .detach();
        view
    }
}

impl Render for NetworkInfoView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (connections, current, select_all, start) =
            state::with(|s| (s.connections.clone(), s.selected_connection, s.selection == Selection::All, s.start_time));
        let count = connections.len();
        if self.index >= count && count > 0 {
            self.index = count - 1;
        }
        let rows_data: Vec<(String, String)> = match connections.get(self.index) {
            Some(conn) => {
                let r = net::if_table().into_iter().find(|r| r.luid == conn.luid).unwrap_or_default();
                let a = self.adapters.iter().find(|a| a.luid == conn.luid).cloned().unwrap_or_default();
                let mac = r.mac.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join("-");
                let (ip, mask) = a.ipv4.first().cloned().unwrap_or_default();
                let elapsed = chrono::Local::now() - start;
                let secs = elapsed.num_seconds().max(0);
                let elapsed_text = tr("IDS_HOUR_MINUTE_SECOND")
                    .replacen("%d", &(secs / 3600).to_string(), 1)
                    .replacen("%d", &((secs / 60) % 60).to_string(), 1)
                    .replacen("%d", &(secs % 60).to_string(), 1);
                let pending = tr("IDS_ACQUIRING");
                vec![
                    (tr("IDS_INTERFACE_NAME"), if a.friendly_name.is_empty() { r.alias.clone() } else { a.friendly_name.clone() }),
                    (tr("IDS_INTERFACE_DESCRIPTION"), r.description.clone()),
                    (tr("IDS_CONNECTION_TYPE"), if_type_text(r.if_type)),
                    (tr("IDS_SPEED"), format!("{}Mbps", r.speed / 1_000_000)),
                    (tr("IDS_ADAPTER_PHYSICAL_ADDRESS"), mac),
                    (tr("IDS_IP_ADDRESS"), ip),
                    (tr("IDS_SUBNET_MASK"), mask),
                    (tr("IDS_DEFAULT_GATEWAY"), a.gateways.join(", ")),
                    ("IPv6".into(), a.ipv6.join(", ")),
                    ("DNS".into(), a.dns.join(", ")),
                    (tr("IDS_OPERATIONAL_STATUS"), oper_status_text(r.oper_status)),
                    (tr("IDS_BYTES_RECEIVED"), bytes_text(r.in_octets)),
                    (tr("IDS_BYTES_SENT"), bytes_text(r.out_octets)),
                    (tr("IDS_BYTES_RECEIVED_SINCE_START"), bytes_text(r.in_octets.saturating_sub(conn.in_bytes_at_start))),
                    (tr("IDS_BYTES_SENT_SINCE_START"), bytes_text(r.out_octets.saturating_sub(conn.out_bytes_at_start))),
                    (tr("IDS_PROGRAM_ELAPSED_TIME"), elapsed_text),
                    (format!("{} (IPv4)", tr("IDS_INTERNET_IP_ADDRESS")), self.internet_v4.clone().unwrap_or_else(|| pending.clone())),
                    (format!("{} (IPv6)", tr("IDS_INTERNET_IP_ADDRESS")), self.internet_v6.clone().unwrap_or(pending)),
                ]
            }
            None => vec![(tr("IDS_NO_CONNECTION"), String::new())],
        };
        let me = cx.entity().downgrade();
        let me2 = me.clone();
        let index = self.index;
        let is_current = index == current && !select_all;
        v_flex()
            .size_full()
            .child(
                v_flex()
                    .id("net-info-list")
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .overflow_y_scrollbar()
                    .child(
                        h_flex()
                            .px_3()
                            .py_1()
                            .gap_3()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(div().w(px(200.)).text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(tr("IDS_ITEM")))
                            .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(tr("IDS_VALUE"))),
                    )
                    .children(rows_data.into_iter().enumerate().map(|(i, (k, v))| {
                        let value: SharedString = v.clone().into();
                        kv_row(k, v, i % 2 == 1, cx)
                            .id(gpui::ElementId::Name(format!("net-row-{i}").into()))
                            .on_double_click(move |_, _, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(value.to_string())))
                    })),
            )
            .child(
                footer(
                    vec![
                        button("net-prev", "<", index == 0, move |_, cx| {
                            if let Some(v) = me.upgrade() {
                                v.update(cx, |v, cx| {
                                    v.index = v.index.saturating_sub(1);
                                    cx.notify();
                                });
                            }
                        }),
                        div()
                            .text_sm()
                            .when(is_current, |d| d.font_weight(gpui::FontWeight::BOLD))
                            .child(format!("{}/{}", (index + 1).min(count.max(1)), count))
                            .into_any_element(),
                        button("net-next", ">", index + 1 >= count, move |_, cx| {
                            if let Some(v) = me2.upgrade() {
                                v.update(cx, |v, cx| {
                                    v.index += 1;
                                    cx.notify();
                                });
                            }
                        }),
                        div().flex_1().into_any_element(),
                        button("net-close", tr("TXT_CLOSE"), false, |w, _| w.remove_window()),
                    ],
                    cx,
                )
                .justify_start(),
            )
    }
}

pub fn open(cx: &mut App) {
    super::open_window(Slot::NetworkInfo, tr("TXT_TITLE_NETWORK_INFO_DLG"), size(px(640.), px(600.)), true, cx, |_, cx| {
        NetworkInfoView::new(cx)
    });
}