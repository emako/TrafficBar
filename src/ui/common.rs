//! Window frame and small form widgets shared by the GPUI windows.

use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use gpui::{
    AnyElement, AnyView, App, Context, Hsla, Image, ImageFormat, InteractiveElement, IntoElement, ParentElement,
    Render, Rgba, SharedString, StatefulInteractiveElement, Styled, Window, WindowControlArea, div, img,
    prelude::FluentBuilder, px, rgb,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Disableable, Icon, IconName, Root, Sizable, h_flex, v_flex};

use crate::i18n::tr;

pub type Handler<T> = Rc<dyn Fn(T, &mut Window, &mut App)>;

fn icon_image(dark: bool) -> Arc<Image> {
    static DARK: OnceLock<Arc<Image>> = OnceLock::new();
    static LIGHT: OnceLock<Arc<Image>> = OnceLock::new();
    if dark {
        DARK.get_or_init(|| {
            Arc::new(Image::from_bytes(ImageFormat::Png, include_bytes!("../assets/icons/notify_dark.png").to_vec()))
        })
        .clone()
    } else {
        LIGHT.get_or_init(|| {
            Arc::new(Image::from_bytes(ImageFormat::Png, include_bytes!("../assets/icons/notify_light.png").to_vec()))
        })
        .clone()
    }
}

/// Title bar + content + gpui-component dialog layer.
pub struct Frame {
    title: SharedString,
    content: AnyView,
}

impl Frame {
    pub fn new(title: SharedString, content: AnyView) -> Self {
        Frame { title, content }
    }
}

impl Render for Frame {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let dark = theme.is_dark();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(px(32.))
                    .flex_shrink_0()
                    .bg(theme.title_bar)
                    .border_b_1()
                    .border_color(theme.title_bar_border)
                    .child(
                        h_flex()
                            .id("title-drag")
                            .flex_1()
                            .h_full()
                            .items_center()
                            .gap_2()
                            .pl_3()
                            .window_control_area(WindowControlArea::Drag)
                            .child(img(icon_image(dark)).size(px(16.)))
                            .child(div().text_sm().child(self.title.clone())),
                    )
                    .child(
                        div()
                            .id("title-close")
                            .w(px(46.))
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(theme.foreground)
                            .hover(|s| s.bg(rgb(0xC42B1C)).text_color(rgb(0xFFFFFF)))
                            .window_control_area(WindowControlArea::Close)
                            .child(Icon::new(IconName::WindowClose).small()),
                    ),
            )
            .child(div().flex_1().min_h_0().child(self.content.clone()))
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

pub fn colorref_to_hsla(c: u32) -> Hsla {
    Rgba {
        r: (c & 0xFF) as f32 / 255.0,
        g: ((c >> 8) & 0xFF) as f32 / 255.0,
        b: ((c >> 16) & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
    .into()
}

pub fn hsla_to_colorref(h: Hsla) -> u32 {
    let c = h.to_rgb();
    let r = (c.r * 255.0).round().clamp(0.0, 255.0) as u32;
    let g = (c.g * 255.0).round().clamp(0.0, 255.0) as u32;
    let b = (c.b * 255.0).round().clamp(0.0, 255.0) as u32;
    r | (g << 8) | (b << 16)
}

/// A titled section (`GROUPBOX`).
pub fn group(title: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let title: SharedString = title.into();
    v_flex()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(title))
}

pub fn row() -> gpui::Div {
    h_flex().gap_2().items_center().flex_wrap()
}

pub fn label(text: impl Into<SharedString>) -> gpui::Div {
    div().text_sm().child(text.into())
}

pub fn muted(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    div().text_xs().text_color(cx.theme().muted_foreground).child(text.into())
}

/// Red "!" that explains the feature needs administrator rights.
pub fn admin_mark(id: impl Into<SharedString>, cx: &App) -> AnyElement {
    let id: SharedString = id.into();
    div()
        .id(gpui::ElementId::Name(id))
        .px_1()
        .text_sm()
        .line_height(px(16.))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(cx.theme().danger)
        .child("!")
        .tooltip(|window, cx| Tooltip::new(tr("BAR_ADMIN_REQUIRED")).build(window, cx))
        .into_any_element()
}

/// Checkbox, followed by an admin mark when `needs_admin` is set and the process is not elevated.
pub fn check(
    id: impl Into<SharedString>,
    text: impl Into<SharedString>,
    checked: bool,
    disabled: bool,
    needs_admin: bool,
    on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let id: SharedString = id.into();
    let elevated = crate::win::is_elevated();
    h_flex()
        .items_center()
        .child(
            Checkbox::new(gpui::ElementId::Name(id.clone()))
                .label(text.into())
                .checked(checked)
                .disabled(disabled)
                .small()
                .on_click(move |v, window, cx| on_change(*v, window, cx)),
        )
        .when(needs_admin && !elevated, |this| this.child(admin_mark(format!("{id}-admin"), cx)))
        .into_any_element()
}

pub fn radio(
    id: impl Into<SharedString>,
    text: impl Into<SharedString>,
    checked: bool,
    disabled: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    gpui_component::radio::Radio::new(gpui::ElementId::Name(id))
        .label(text.into())
        .checked(checked)
        .disabled(disabled)
        .small()
        .on_click(move |_, window, cx| on_click(window, cx))
        .into_any_element()
}

/// `- value +` integer spin control (`CSpinEdit`).
pub fn stepper(
    id: impl Into<SharedString>,
    value: i32,
    min: i32,
    max: i32,
    step: i32,
    disabled: bool,
    on_change: impl Fn(i32, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let id: SharedString = id.into();
    let on_change: Handler<i32> = Rc::new(on_change);
    let dec = on_change.clone();
    let inc = on_change.clone();
    h_flex()
        .items_center()
        .border_1()
        .border_color(cx.theme().input)
        .rounded(cx.theme().radius)
        .child(
            Button::new(gpui::ElementId::Name(format!("{id}-dec").into()))
                .icon(IconName::Minus)
                .ghost()
                .xsmall()
                .disabled(disabled || value <= min)
                .on_click(move |_, window, cx| dec((value - step).max(min), window, cx)),
        )
        .child(
            div()
                .min_w(px(44.))
                .px_1()
                .text_sm()
                .text_center()
                .when(disabled, |d| d.text_color(cx.theme().muted_foreground))
                .child(value.to_string()),
        )
        .child(
            Button::new(gpui::ElementId::Name(format!("{id}-inc").into()))
                .icon(IconName::Plus)
                .ghost()
                .xsmall()
                .disabled(disabled || value >= max)
                .on_click(move |_, window, cx| inc((value + step).min(max), window, cx)),
        )
        .into_any_element()
}

/// Drop-down list (`CBS_DROPDOWNLIST`).
pub fn choice(
    id: impl Into<SharedString>,
    options: Vec<String>,
    selected: usize,
    disabled: bool,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let current = options.get(selected).cloned().unwrap_or_default();
    let on_select: Handler<usize> = Rc::new(on_select);
    Button::new(gpui::ElementId::Name(id))
        .label(current)
        .outline()
        .small()
        .dropdown_caret(true)
        .disabled(disabled)
        .dropdown_menu(move |mut menu, _window, _cx| {
            for (i, opt) in options.iter().enumerate() {
                let f = on_select.clone();
                menu = menu.item(
                    PopupMenuItem::new(opt.clone())
                        .checked(i == selected)
                        .on_click(move |_, window, cx| f(i, window, cx)),
                );
            }
            menu.max_h(px(320.)).scrollable(true)
        })
        .into_any_element()
}

pub fn button(
    id: impl Into<SharedString>,
    text: impl Into<SharedString>,
    disabled: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    Button::new(gpui::ElementId::Name(id))
        .label(text.into())
        .small()
        .outline()
        .disabled(disabled)
        .on_click(move |_, window, cx| on_click(window, cx))
        .into_any_element()
}

pub fn primary_button(
    id: impl Into<SharedString>,
    text: impl Into<SharedString>,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    Button::new(gpui::ElementId::Name(id))
        .label(text.into())
        .small()
        .primary()
        .on_click(move |_, window, cx| on_click(window, cx))
        .into_any_element()
}

/// Footer with right-aligned buttons.
pub fn footer(children: Vec<AnyElement>, cx: &App) -> gpui::Div {
    h_flex()
        .flex_shrink_0()
        .justify_end()
        .gap_2()
        .px_4()
        .py_3()
        .border_t_1()
        .border_color(cx.theme().border)
        .children(children)
}

/// Two-column key/value list used by the details windows.
pub fn kv_row(key: impl Into<SharedString>, value: impl Into<SharedString>, odd: bool, cx: &App) -> gpui::Div {
    h_flex()
        .w_full()
        .px_3()
        .py_1()
        .gap_3()
        .when(odd, |d| d.bg(cx.theme().muted.opacity(0.4)))
        .child(div().w(px(200.)).flex_shrink_0().text_sm().text_color(cx.theme().muted_foreground).child(key.into()))
        .child(div().flex_1().min_w_0().text_sm().child(value.into()))
}
