//! "检查更新" (`CUpdateHelper` + `CTrafficMonitorApp::CheckUpdate`), against this program's own
//! release feed. The URLs can be overridden in config.ini (`[app] update_url_github/gitee`).

use gpui::{App, AppContext, Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div, px, size};
use gpui_component::{scroll::ScrollableElement, v_flex};

use super::common::{button, footer, primary_button};
use super::Slot;
use crate::i18n::tr;
use crate::state;

const DEFAULT_GITHUB: &str = "https://api.github.com/repos/emako/TrafficMonitorBar/releases/latest";
const DEFAULT_GITEE: &str = "https://gitee.com/api/v5/repos/emako/TrafficMonitorBar/releases/latest";

#[derive(Clone)]
enum Outcome {
    Failed,
    UpToDate,
    Available { version: String, url: String, notes: String },
}

fn parse_version(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['.', '-'])
        .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0))
        .collect()
}

fn newer(remote: &str, local: &str) -> bool {
    let (mut r, mut l) = (parse_version(remote), parse_version(local));
    let n = r.len().max(l.len());
    r.resize(n, 0);
    l.resize(n, 0);
    r > l
}

fn fetch(url: &str) -> Outcome {
    let ua = format!("TrafficMonitorBar/{}", env!("CARGO_PKG_VERSION"));
    let Some(body) = crate::http::get(url, &ua) else {
        return Outcome::Failed;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Outcome::Failed;
    };
    let Some(tag) = v.get("tag_name").and_then(|t| t.as_str()) else {
        return Outcome::Failed;
    };
    if !newer(tag, env!("CARGO_PKG_VERSION")) {
        return Outcome::UpToDate;
    }
    Outcome::Available {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        url: v.get("html_url").and_then(|u| u.as_str()).unwrap_or(url).to_string(),
        notes: v.get("body").and_then(|b| b.as_str()).unwrap_or("").to_string(),
    }
}

pub fn check(silent: bool, cx: &mut App) {
    let already = state::with_mut(|s| std::mem::replace(&mut s.checking_update, true));
    if already {
        return;
    }
    let url = state::with(|s| {
        let ini = crate::config::ini::Ini::load(&s.settings.config_path());
        if s.settings.general.update_source == 1 {
            ini.get_string("app", "update_url_gitee", DEFAULT_GITEE)
        } else {
            ini.get_string("app", "update_url_github", DEFAULT_GITHUB)
        }
    });
    cx.spawn(async move |cx| {
        let outcome = cx.background_spawn(async move { fetch(&url) }).await;
        let _ = cx.update(|cx| {
            state::with_mut(|s| s.checking_update = false);
            if silent && !matches!(outcome, Outcome::Available { .. }) {
                return;
            }
            super::close(Slot::Update, cx);
            super::open_window(Slot::Update, tr("IDS_VERSION_UPDATE"), size(px(480.), px(320.)), false, cx, move |_, _| UpdateView {
                outcome: outcome.clone(),
            });
        });
    })
    .detach();
}

pub struct UpdateView {
    outcome: Outcome,
}

impl Render for UpdateView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, url) = match &self.outcome {
            Outcome::Failed => (tr("IDS_CHECK_UPDATE_FAILD"), None),
            Outcome::UpToDate => (
                format!("{}\n{}{}", tr("IDS_ALREADY_UPDATED"), tr("BAR_CURRENT_VERSION"), env!("CARGO_PKG_VERSION")),
                None,
            ),
            Outcome::Available { version, url, notes } => {
                let text = if notes.trim().is_empty() {
                    tr("IDS_UPDATE_AVLIABLE").replace("%s", version)
                } else {
                    tr("IDS_UPDATE_AVLIABLE2").replacen("%s", version, 1).replacen("%s", notes.trim(), 1)
                };
                (text.replace("\r\n", "\n"), Some(url.clone()))
            }
        };
        let mut buttons = Vec::new();
        if let Some(url) = url {
            buttons.push(primary_button("update-go", tr("BAR_DOWNLOAD"), move |w, _| {
                crate::win::shell_open(&url);
                w.remove_window();
            }));
        }
        buttons.push(button("update-close", tr("TXT_CLOSE"), false, |w, _| w.remove_window()));
        v_flex()
            .size_full()
            .child(
                v_flex()
                    .id("update-text")
                    .flex_1()
                    .min_h_0()
                    .p_4()
                    .overflow_y_scrollbar()
                    .children(text.split('\n').map(|l| div().text_sm().child(l.to_string()))),
            )
            .child(footer(buttons, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn version_compare() {
        assert!(newer("v0.2.0", "0.1.9"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(newer("1.0", "0.9.9"));
    }
}
