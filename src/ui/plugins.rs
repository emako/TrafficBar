//! Plugin manager (`CPluginManagerDlg`) and plugin details (`CPluginInfoDlg`).

use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Window,
    div, prelude::FluentBuilder, px, size,
};
use gpui_component::link::Link;
use gpui_component::{ActiveTheme, h_flex, scroll::ScrollableElement, v_flex};

use super::common::{button, footer, kv_row, muted};
use super::Slot;
use crate::i18n::{tr, tr_plain};
use crate::plugin::PluginState;
use crate::state;

fn status_text(p: &crate::plugin::PluginInfo) -> String {
    match p.state {
        PluginState::Succeed => tr("IDS_PLUGIN_LOAD_SUCCEED"),
        PluginState::ModuleLoadFailed => tr("IDS_PLUGIN_MODULE_LOAD_FAILED").replace("<%1%>", &p.error_code.to_string()),
        PluginState::FunctionGetFailed => tr("IDS_PLUGIN_FUNCTION_GET_FAILED").replace("<%1%>", &p.error_code.to_string()),
        PluginState::VersionNotSupport => tr("IDS_PLUGIN_VERSION_NOT_SUPPORT"),
        PluginState::Disabled => tr("IDS_DISABLED"),
    }
}

pub struct PluginManagerView {
    selected: Option<usize>,
}

impl Render for PluginManagerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (plugins, disabled) = state::with(|s| (s.plugins.plugins.clone(), s.settings.cfg.plugin_disabled.clone()));
        let me = cx.entity().downgrade();
        let selected = self.selected.filter(|i| *i < plugins.len());
        let sel_plugin = selected.and_then(|i| plugins.get(i).cloned());
        let header = h_flex()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(div().w(px(160.)).child(tr("IDS_FILE_NAME")))
            .child(div().w(px(160.)).child(tr("IDS_PLUGIN_NAME")))
            .child(div().w(px(80.)).child(tr("IDS_VERSION")))
            .child(div().flex_1().child(tr("IDS_STATUS")));
        let rows = plugins.iter().enumerate().map(|(i, p)| {
            let me = me.clone();
            let is_sel = selected == Some(i);
            let pending_disable = disabled.iter().any(|d| d.eq_ignore_ascii_case(&p.file_name));
            h_flex()
                .id(gpui::ElementId::Name(format!("plugin-row-{i}").into()))
                .px_2()
                .py_1()
                .text_sm()
                .when(is_sel, |d| d.bg(cx.theme().accent))
                .on_click(move |_, _, cx| {
                    if let Some(v) = me.upgrade() {
                        v.update(cx, |v, cx| {
                            v.selected = Some(i);
                            cx.notify();
                        });
                    }
                })
                .child(div().w(px(160.)).child(p.file_name.clone()))
                .child(div().w(px(160.)).child(p.props[0].clone()))
                .child(div().w(px(80.)).child(p.props[4].clone()))
                .child(div().flex_1().child(if pending_disable && p.state != PluginState::Disabled {
                    format!("{} ({})", status_text(p), tr("IDS_DISABLED"))
                } else {
                    status_text(p)
                }))
        });
        let loaded = sel_plugin.as_ref().map(|p| p.is_loaded()).unwrap_or(false);
        let sel_disabled = sel_plugin
            .as_ref()
            .map(|p| disabled.iter().any(|d| d.eq_ignore_ascii_case(&p.file_name)))
            .unwrap_or(false);
        let file_name = sel_plugin.as_ref().map(|p| p.file_name.clone()).unwrap_or_default();
        let plugin_dir = crate::win::program_dir().join("plugins");
        let hwnd = state::with(|s| s.main_hwnd);
        v_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .gap_2()
                    .child(header)
                    .child(
                        v_flex()
                            .id("plugin-list")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .children(rows)
                            .when(plugins.is_empty(), |d| d.child(muted(tr("BAR_NO_PLUGINS"), cx))),
                    )
                    .child(
                        h_flex()
                            .gap_4()
                            .child(Link::new("plugin-download").href("https://github.com/zhongyang219/TrafficMonitorPlugins/blob/main/download/plugin_download.md").child(tr("TXT_PLUGIN_DOWNLOAD_STATIC")))
                            .child(Link::new("plugin-dev").href("https://github.com/zhongyang219/TrafficMonitor/wiki/插件开发指南").child(tr("TXT_PLUGIN_DEV_GUID_STATIC")))
                            .child(
                                div()
                                    .id("plugin-open-dir")
                                    .text_sm()
                                    .text_color(cx.theme().link)
                                    .cursor_pointer()
                                    .child(tr("TXT_OPEN_PLUGIN_DIR_STATIC"))
                                    .on_click(move |_, _, _| {
                                        let _ = std::fs::create_dir_all(&plugin_dir);
                                        crate::win::shell_open(&plugin_dir.to_string_lossy());
                                    }),
                            ),
                    )
                    .child(muted(tr("BAR_PLUGIN_TEXT_ONLY"), cx)),
            )
            .child(footer(
                vec![
                    button("plugin-info", tr_plain("TXT_PLUGIN_INFO_BUTTON"), selected.is_none(), move |_, cx| {
                        if let Some(i) = selected {
                            open_info(i, cx);
                        }
                    }),
                    button("plugin-options", tr_plain("TXT_OPTINS_BUTTON"), !loaded, move |_, _| {
                        if let Some(i) = selected {
                            crate::app::show_plugin_options(i, hwnd);
                        }
                    }),
                    button(
                        "plugin-disable",
                        if sel_disabled { crate::i18n::strip_mnemonic(&tr("TXT_PLUGIN_DISABLE")) + " ✓" } else { crate::i18n::strip_mnemonic(&tr("TXT_PLUGIN_DISABLE")) },
                        selected.is_none(),
                        move |w, cx| {
                            if file_name.is_empty() {
                                return;
                            }
                            let name = file_name.clone();
                            state::with_mut(|s| {
                                let d = &mut s.settings.cfg.plugin_disabled;
                                if d.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
                                    d.retain(|x| !x.eq_ignore_ascii_case(&name));
                                } else {
                                    d.push(name.clone());
                                }
                                s.save_settings();
                            });
                            let text = tr("IDS_RESTART_TO_APPLY_CHANGE_INFO");
                            gpui_component::WindowExt::open_dialog(w, cx, move |dialog, _, _| dialog.alert().title(tr("APP_NAME")).child(text.clone()));
                        },
                    ),
                    div().flex_1().into_any_element(),
                    button("plugin-close", tr("TXT_CLOSE"), false, |w, _| w.remove_window()),
                ],
                cx,
            ))
    }
}

pub fn open_manager(cx: &mut App) {
    super::open_window(Slot::PluginManager, tr("TXT_TITLE_PLUGIN_MANAGE"), size(px(640.), px(440.)), true, cx, |_, _| {
        PluginManagerView { selected: None }
    });
}

pub struct PluginInfoView {
    index: usize,
}

impl Render for PluginInfoView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let info = state::with(|s| {
            let p = s.plugins.plugins.get(self.index)?.clone();
            let names: Vec<String> = s.plugins.items.iter().filter(|i| p.item_ids.contains(&i.id)).map(|i| i.name.clone()).collect();
            Some((p, names))
        });
        let rows: Vec<(String, String)> = match info {
            Some((p, names)) => vec![
                (tr("IDS_NAME"), p.props[0].clone()),
                (tr("IDS_DESCRIPTION"), p.props[1].clone()),
                (tr("IDS_FILE_NAME"), p.file_name.clone()),
                (tr("IDS_FILE_PATH"), p.file_path.to_string_lossy().into_owned()),
                (tr("IDS_ITEM_NUM"), p.item_ids.len().to_string()),
                (tr("IDS_ITEM_NAMES"), names.join("; ")),
                (tr("IDS_DISP_ITEM_ID"), p.item_ids.join("; ")),
                (tr("IDS_AUTHOR"), p.props[2].clone()),
                (tr("IDS_COPYRIGHT"), p.props[3].clone()),
                (tr("IDS_VERSION"), p.props[4].clone()),
                (tr("IDS_URL"), p.props[5].clone()),
                (tr("IDS_PLUGIN_API_VERSION"), p.api_version.to_string()),
            ],
            None => Vec::new(),
        };
        v_flex()
            .size_full()
            .child(
                v_flex()
                    .id("plugin-info")
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .overflow_y_scrollbar()
                    .children(rows.into_iter().enumerate().map(|(i, (k, v))| kv_row(k, v, i % 2 == 1, cx))),
            )
            .child(footer(vec![button("plugin-info-close", tr("TXT_CLOSE"), false, |w, _| w.remove_window())], cx))
    }
}

pub fn open_info(index: usize, cx: &mut App) {
    super::close(Slot::PluginInfo, cx);
    super::open_window(Slot::PluginInfo, tr("IDS_PLUGIN_INFO"), size(px(560.), px(460.)), true, cx, move |_, _| {
        PluginInfoView { index }
    });
}
