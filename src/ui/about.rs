//! About (`CAboutDlg`).

use gpui::{App, Context, IntoElement, ParentElement, Render, Styled, Window, div, px, size};
use gpui_component::link::Link;
use gpui_component::{ActiveTheme, v_flex};

use super::common::{button, footer, muted};
use super::Slot;
use crate::i18n::tr;

pub struct AboutView;

fn link(id: &'static str, text: String, url: &'static str) -> impl IntoElement {
    Link::new(id).href(url).text_sm().child(text)
}

impl Render for AboutView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let elevated = crate::win::is_elevated();
        let v = crate::win::windows_version();
        v_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_1()
                    .p_5()
                    .gap_2()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(format!("TrafficBar {}", env!("CARGO_PKG_VERSION"))),
                    )
                    .child(div().text_sm().child(tr("BAR_BASED_ON")))
                    .child(muted("Copyright (C) 2017-2026 By ZhongYang", cx))
                    .child(muted(
                        format!(
                            "Windows {}.{} build {}{}",
                            v.major,
                            v.minor,
                            v.build,
                            if elevated { format!(" ({})", "Administrator") } else { String::new() }
                        ),
                        cx,
                    ))
                    .child(div().h(px(8.)))
                    .child(div().text_sm().child(tr("TXT_ABOUT_THIRD_PARTY_LIB")))
                    .child(link("lib-gpui", "GPUI".into(), "https://github.com/zed-industries/zed/tree/main/crates/gpui"))
                    .child(link("lib-gpui-component", "gpui-component".into(), "https://github.com/longbridge/gpui-component"))
                    .child(link("lib-lhm", "LibreHardwareMonitor".into(), "https://github.com/LibreHardwareMonitor/LibreHardwareMonitor"))
                    .child(div().h(px(8.)))
                    .child(link("about-github", tr("BAR_SOURCE_CODE"), "https://github.com/zhongyang219/TrafficMonitor"))
                    .child(link("about-license", tr("TXT_ABOUT_LICENSE"), "https://github.com/zhongyang219/TrafficMonitor/blob/master/LICENSE"))
                    .child(div().h(px(8.)))
                    .child(muted(tr("BAR_PLUGIN_TEXT_ONLY"), cx))
                    .text_color(cx.theme().foreground),
            )
            .child(footer(vec![button("about-close", tr("TXT_CLOSE"), false, |w, _| w.remove_window())], cx))
    }
}

pub fn open(cx: &mut App) {
    let title = tr("TXT_TITLE_ABOUT").replace("TrafficMonitor", "TrafficBar");
    super::open_window(Slot::About, title, size(px(480.), px(480.)), false, cx, |_, _| AboutView);
}
