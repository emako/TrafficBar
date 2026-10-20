//! Notify icon picker (`CIconSelectDlg`).

use std::sync::Arc;

use gpui::{
    App, Context, Image, ImageFormat, IntoElement, ParentElement, Render, SharedString, Styled,
    WeakEntity, Window, div, img, px, rgb, size,
};
use gpui_component::{v_flex, ActiveTheme};

use super::Slot;
use super::common::{button, check, choice, footer, group, label, primary_button, row};
use crate::i18n::{tr, tr_plain};
use crate::notify_icon as ico;

pub struct IconSelectView {
    names: Vec<String>,
    index: usize,
    auto_adapt: bool,
    win10: bool,
    preview: Arc<Image>,
    light_preview: bool,
}

impl IconSelectView {
    fn new() -> Self {
        let names = ico::names();
        let (selected, auto) = crate::state::with(|s| {
            (
                s.settings.general.notify_icon.clone(),
                s.settings.general.notify_icon_auto_adapt,
            )
        });
        let index = names.iter().position(|n| n == &selected).unwrap_or(0);
        let name = names.get(index).cloned().unwrap_or_default();
        Self {
            preview: preview_image(&name),
            light_preview: name_is_light(&name),
            names,
            index,
            auto_adapt: auto,
            win10: crate::win::windows_version().is_windows10_or_later(),
        }
    }

    fn select(&mut self, index: usize) {
        self.index = index;
        let name = self.names.get(index).cloned().unwrap_or_default();
        self.light_preview = name_is_light(&name);
        self.preview = preview_image(&name);
    }
}

fn name_is_light(name: &str) -> bool {
    name.trim_end_matches(".ico").ends_with("_light")
}

fn label_of(name: &str) -> String {
    name.trim_end_matches(".ico").to_string()
}

fn preview_image(name: &str) -> Arc<Image> {
    let bytes = ico::preview_png(name).unwrap_or_else(|| {
        include_bytes!("../assets/icons/notify_dark.png").to_vec()
    });
    Arc::new(Image::from_bytes(ImageFormat::Png, bytes))
}

impl Render for IconSelectView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let labels: Vec<String> = self.names.iter().map(|n| label_of(n)).collect();
        let index = self.index;
        let auto = self.auto_adapt;
        let win10 = self.win10;
        let me = cx.entity().downgrade();
        let ok = me.clone();
        let bg = if self.light_preview {
            rgb(0xF3F3F3)
        } else {
            rgb(0x1F1F1F)
        };
        v_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_1()
                    .p_4()
                    .gap_3()
                    .child(row().child(label(tr("TXT_SELECT_A_ICON"))).child(choice(
                        "notify-icon",
                        labels,
                        index,
                        false,
                        move |i, _, cx| {
                            let _ = me.update(cx, |this, cx| {
                                this.select(i);
                                cx.notify();
                            });
                        },
                    )))
                    .child(
                        group(tr("TXT_PREVIEW"), cx).child(
                            div()
                                .w(px(48.))
                                .h(px(48.))
                                .rounded(cx.theme().radius)
                                .bg(bg)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(img(self.preview.clone()).size(px(32.))),
                        ),
                    )
                    .child(check(
                        "notify-auto",
                        tr("TXT_NOTIFY_ICON_AUTO_ADAPT_CHECK"),
                        auto,
                        !win10,
                        false,
                        {
                            let me = ok.clone();
                            move |v, _, cx| {
                                let _ = me.update(cx, |this, cx| {
                                    this.auto_adapt = v;
                                    cx.notify();
                                });
                            }
                        },
                        cx,
                    )),
            )
            .child(footer(
                vec![
                    primary_button("icon-ok", tr_plain("TXT_OK"), move |window, cx| {
                        commit(&ok, cx);
                        window.remove_window();
                    }),
                    button("icon-cancel", tr_plain("TXT_CANCEL"), false, |window, _| {
                        window.remove_window()
                    }),
                ],
                cx,
            ))
    }
}

fn commit(view: &WeakEntity<IconSelectView>, cx: &mut App) {
    let Ok((name, mut auto)) = view.update(cx, |v, _| {
        (
            v.names
                .get(v.index)
                .cloned()
                .unwrap_or_else(|| "notify_dark.ico".into()),
            v.auto_adapt,
        )
    }) else {
        return;
    };
    if !crate::win::windows_version().is_windows10_or_later() {
        auto = false;
    }
    let light = crate::state::with(|s| s.rt.light_theme);
    let name = if auto { ico::adapt(&name, light) } else { name };
    let mut settings = crate::state::with(|s| s.settings.clone());
    settings.general.notify_icon = name;
    settings.general.notify_icon_auto_adapt = auto;
    crate::app::apply_settings(settings);
}

pub fn open(cx: &mut App) {
    super::open_window(
        Slot::IconSelect,
        SharedString::from(tr("TXT_TITLE_CHANGE_ICON")),
        size(px(420.), px(280.)),
        false,
        cx,
        |_, _| IconSelectView::new(),
    );
}
