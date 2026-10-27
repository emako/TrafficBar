//! In-app alert and confirm dialogs (`gpui-component` dialog), shared by every former
//! system message box. When no settings window is open, a small host window carries the dialog.

use std::rc::Rc;

use gpui::{
    AnyWindowHandle, App, Context, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, div, px, size,
};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::scroll::ScrollableElement;
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable, WindowExt, h_flex, v_flex,
};

use super::Slot;
use crate::i18n::{tr, tr_plain};

struct Host;

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

/// One-button notice on `window`.
pub fn alert(window: &mut Window, cx: &mut App, text: impl Into<String>, warning: bool) {
    present(window, cx, text.into(), warning, false, false, None);
}

/// Ok / Cancel on `window`. `on_ok` runs only when the user confirms.
pub fn confirm(
    window: &mut Window,
    cx: &mut App,
    text: impl Into<String>,
    warning: bool,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
) {
    present(
        window,
        cx,
        text.into(),
        warning,
        true,
        false,
        Some(Rc::new(on_ok)),
    );
}

pub fn show_alert(text: String, warning: bool, cx: &mut App) {
    let Some((handle, dismiss)) = attach(cx) else {
        log::warn!("alert dropped, no window: {text}");
        return;
    };
    let _ = handle.update(cx, |_, window, cx| {
        present(window, cx, text, warning, false, dismiss, None);
    });
}

pub fn show_confirm(
    text: String,
    warning: bool,
    cx: &mut App,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
) {
    let Some((handle, dismiss)) = attach(cx) else {
        log::warn!("confirm dropped, no window: {text}");
        return;
    };
    let on_ok = Rc::new(on_ok);
    let _ = handle.update(cx, |_, window, cx| {
        present(window, cx, text, warning, true, dismiss, Some(on_ok));
    });
}

fn present(
    window: &mut Window,
    cx: &mut App,
    text: String,
    warning: bool,
    confirm: bool,
    dismiss_window: bool,
    on_ok: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let mut dialog = dialog
            .title(tr("APP_NAME"))
            .w(px(440.))
            .child(body(&text, warning, cx))
            .button_props(
                DialogButtonProps::default()
                    .ok_text(tr_plain("TXT_OK"))
                    .cancel_text(tr_plain("TXT_CANCEL")),
            )
            .on_close(move |_, window, cx| {
                if !dismiss_window {
                    return;
                }
                window.defer(cx, |window, cx| {
                    if !window.has_active_dialog(cx) {
                        window.remove_window();
                    }
                });
            });
        if confirm {
            let on_ok = on_ok.clone();
            dialog = dialog.confirm().on_ok(move |_, window, cx| {
                if let Some(on_ok) = &on_ok {
                    on_ok(window, cx);
                }
                true
            });
        } else {
            dialog = dialog.alert();
        }
        dialog
    });
}

fn body(text: &str, warning: bool, cx: &App) -> gpui::AnyElement {
    let color = if warning {
        cx.theme().warning
    } else {
        cx.theme().info
    };
    let icon = if warning {
        IconName::TriangleAlert
    } else {
        IconName::Info
    };
    h_flex()
        .w_full()
        .items_start()
        .gap_3()
        .child(Icon::new(icon).large().text_color(color))
        .child(
            v_flex()
                .id("prompt-body")
                .flex_1()
                .min_w_0()
                .max_h(px(220.))
                .overflow_y_scrollbar()
                .gap_1()
                .children(lines(text)),
        )
        .into_any_element()
}

fn lines(text: &str) -> Vec<gpui::Div> {
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                div().h(px(8.))
            } else {
                div()
                    .w_full()
                    .text_sm()
                    .whitespace_normal()
                    .line_height(px(20.))
                    .child(SharedString::from(line.to_string()))
            }
        })
        .collect()
}

/// Front settings window, or a host created for this dialog.
/// The bool is true when dismissing the dialog should close that host.
fn attach(cx: &mut App) -> Option<(AnyWindowHandle, bool)> {
    super::prune(cx);
    let active = cx.active_window();
    let windows = cx.global::<super::OpenWindows>().windows.clone();

    let mut chosen: Option<(AnyWindowHandle, isize, bool)> = None;
    if let Some(active) = active {
        if let Some(w) = windows.iter().find(|w| w.1 == active && w.0 != Slot::Prompt) {
            chosen = Some((w.1, w.2, false));
        }
    }
    if chosen.is_none() {
        if let Some(w) = windows.iter().rev().find(|w| w.0 != Slot::Prompt) {
            chosen = Some((w.1, w.2, false));
        }
    }
    if chosen.is_none() {
        if let Some(w) = windows.iter().find(|w| w.0 == Slot::Prompt) {
            chosen = Some((w.1, w.2, true));
        }
    }
    if chosen.is_none() {
        super::open_window(
            Slot::Prompt,
            tr("APP_NAME"),
            size(px(500.), px(440.)),
            false,
            cx,
            |_, _| Host,
        );
        if let Some(w) = cx
            .global::<super::OpenWindows>()
            .windows
            .iter()
            .find(|w| w.0 == Slot::Prompt)
        {
            chosen = Some((w.1, w.2, true));
        }
    }
    let (handle, hwnd, dismiss) = chosen?;
    super::bring_to_front(handle, hwnd, cx);
    Some((handle, dismiss))
}
