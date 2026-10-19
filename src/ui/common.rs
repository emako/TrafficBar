//! Window frame and small form widgets shared by the GPUI windows.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use gpui::{
    AnyElement, AnyView, App, AppContext, Context, Entity, Focusable, Hsla, Image, ImageFormat,
    InteractiveElement, IntoElement, ParentElement, Render, Rgba, SharedString,
    StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, WindowControlArea, div,
    img, prelude::FluentBuilder, px, relative, rems, rgb,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState, SelectAll};
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
            Arc::new(Image::from_bytes(
                ImageFormat::Png,
                include_bytes!("../assets/icons/notify_dark.png").to_vec(),
            ))
        })
        .clone()
    } else {
        LIGHT
            .get_or_init(|| {
                Arc::new(Image::from_bytes(
                    ImageFormat::Png,
                    include_bytes!("../assets/icons/notify_light.png").to_vec(),
                ))
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
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title),
        )
}

pub fn row() -> gpui::Div {
    h_flex().gap_2().items_center().flex_wrap()
}

pub fn label(text: impl Into<SharedString>) -> gpui::Div {
    div().text_sm().child(text.into())
}

pub fn muted(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
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
        .when(needs_admin && !elevated, |this| {
            this.child(admin_mark(format!("{id}-admin"), cx))
        })
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

/// Keeps one input per stepper id so typed text is not wiped on every redraw.
#[derive(Default)]
pub struct StepperSet {
    items: HashMap<SharedString, Entity<Stepper>>,
}

impl StepperSet {
    fn upsert(
        &mut self,
        id: SharedString,
        value: i32,
        min: i32,
        max: i32,
        step: i32,
        disabled: bool,
        on_change: Handler<i32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Stepper> {
        if let Some(entity) = self.items.get(&id).cloned() {
            let push = entity.update(cx, |stepper, cx| {
                stepper.sync(value, min, max, step, disabled, on_change, window, cx)
            });
            if let Some(text) = push {
                let input = entity.read(cx).input.clone();
                input.update(cx, |input, cx| input.set_value(text, window, cx));
            }
            return entity;
        }
        let entity = cx.new(|cx| {
            Stepper::new(
                id.clone(),
                value,
                min,
                max,
                step,
                disabled,
                on_change,
                window,
                cx,
            )
        });
        self.items.insert(id, entity.clone());
        entity
    }
}

/// `- value +` integer spin control (`CSpinEdit`). The middle value can be typed.
///
/// In-range numbers apply immediately. Empty text, a lone minus, and out-of-range
/// numbers stay on screen until the field blurs, then the value is clamped.
pub fn stepper(
    host: &Entity<StepperSet>,
    id: impl Into<SharedString>,
    value: i32,
    min: i32,
    max: i32,
    step: i32,
    disabled: bool,
    on_change: impl Fn(i32, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    host.update(cx, |set, cx| {
        set.upsert(
            id.into(),
            value,
            min,
            max,
            step,
            disabled,
            Rc::new(on_change),
            window,
            cx,
        )
    })
    .into_any_element()
}

struct Stepper {
    id: SharedString,
    input: Entity<InputState>,
    value: i32,
    min: i32,
    max: i32,
    step: i32,
    disabled: bool,
    allow_neg: Rc<Cell<bool>>,
    on_change: Handler<i32>,
    _sub: Subscription,
}

impl Stepper {
    fn new(
        id: SharedString,
        value: i32,
        min: i32,
        max: i32,
        step: i32,
        disabled: bool,
        on_change: Handler<i32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let allow_neg = Rc::new(Cell::new(min < 0));
        let flag = allow_neg.clone();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(value.to_string())
                .validate(move |text, _| integer_text(text, flag.get()))
        });
        let _sub = cx.subscribe_in(&input, window, |this, _, ev, window, cx| match ev {
            InputEvent::Change => this.on_edit(true, window, cx),
            InputEvent::Blur | InputEvent::PressEnter { .. } => this.on_edit(false, window, cx),
            InputEvent::Focus => window.dispatch_action(Box::new(SelectAll), cx),
        });
        Self {
            id,
            input,
            value,
            min,
            max,
            step: step.max(1),
            disabled,
            allow_neg,
            on_change,
            _sub,
        }
    }

    /// Push `value` into the field only while the user is not editing it.
    fn sync(
        &mut self,
        value: i32,
        min: i32,
        max: i32,
        step: i32,
        disabled: bool,
        on_change: Handler<i32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        self.min = min;
        self.max = max;
        self.step = step.max(1);
        self.disabled = disabled;
        self.on_change = on_change;
        self.allow_neg.set(min < 0);
        if self.input.read(cx).focus_handle(cx).is_focused(window) {
            return None;
        }
        self.value = value;
        let desired = value.to_string();
        let current = self.input.read(cx).value().to_string();
        (current != desired).then_some(desired)
    }

    fn on_edit(&mut self, live: bool, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        let next = if live {
            let Some(v) = parsed_in_range(&text, self.min, self.max) else {
                return;
            };
            v
        } else {
            commit_value(&text, self.min, self.max, self.value)
        };
        if next != self.value {
            self.value = next;
            let on_change = self.on_change.clone();
            on_change(next, window, cx);
        }
        if !live {
            let normalized = self.value.to_string();
            if text != normalized {
                self.input
                    .update(cx, |input, cx| input.set_value(normalized, window, cx));
            }
        }
    }
}

impl Render for Stepper {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = self.input.read(cx).value().to_string();
        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let shown = text.parse::<i32>().unwrap_or(self.value);
        let base = shown.clamp(self.min, self.max);
        let width = field_width(self.min, self.max);
        let pad = center_pad(width, text_width(&text, window));
        let this = cx.entity().downgrade();
        let dec = this.clone();
        let id = self.id.clone();
        let border = if focused {
            cx.theme().ring
        } else {
            cx.theme().input
        };
        h_flex()
            .flex_shrink_0()
            .items_center()
            .overflow_hidden()
            .border_1()
            .border_color(border)
            .rounded(cx.theme().radius)
            .child(
                Button::new(gpui::ElementId::Name(format!("{id}-dec").into()))
                    .icon(IconName::Minus)
                    .ghost()
                    .xsmall()
                    .tab_stop(false)
                    .disabled(self.disabled || base <= self.min)
                    .on_click(move |_, window, cx| bump(&dec, -1, window, cx)),
            )
            .child(
                div().w(width).flex_shrink_0().child(
                    Input::new(&self.input)
                        .appearance(false)
                        .small()
                        // Input's small size still uses a 1.25rem line box, so the digits
                        // (and the selection behind them) read larger than the text_sm labels.
                        .text_xs()
                        .line_height(relative(1.))
                        .h(rems(1.25))
                        .pl(pad)
                        .pr(px(0.))
                        .disabled(self.disabled)
                        .when(self.disabled, |this| {
                            this.text_color(cx.theme().muted_foreground)
                        }),
                ),
            )
            .child(
                Button::new(gpui::ElementId::Name(format!("{id}-inc").into()))
                    .icon(IconName::Plus)
                    .ghost()
                    .xsmall()
                    .tab_stop(false)
                    .disabled(self.disabled || base >= self.max)
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| bump(&this, 1, window, cx)
                    }),
            )
    }
}

fn bump(this: &WeakEntity<Stepper>, dir: i32, window: &mut Window, cx: &mut App) {
    let Some(this) = this.upgrade() else { return };
    let Some((next, changed, on_change, input)) = this.update(cx, |stepper, cx| {
        if stepper.disabled {
            return None;
        }
        let text = stepper.input.read(cx).value().to_string();
        let next = step_value(
            &text,
            stepper.value,
            stepper.min,
            stepper.max,
            stepper.step,
            dir,
        );
        if next == stepper.value && text == next.to_string() {
            return None;
        }
        let changed = next != stepper.value;
        stepper.value = next;
        Some((
            next,
            changed,
            stepper.on_change.clone(),
            stepper.input.clone(),
        ))
    }) else {
        return;
    };
    if changed {
        on_change(next, window, cx);
    }
    input.update(cx, |input, cx| {
        input.set_value(next.to_string(), window, cx)
    });
}

fn field_width(min: i32, max: i32) -> gpui::Pixels {
    let chars = min.to_string().len().max(max.to_string().len()).max(2);
    px((chars as f32 * 8.0 + 24.0).max(44.))
}

/// Left inset that places `text_width` in the middle of `field`.
///
/// The input paints glyphs from the left of its content box, so equal padding would
/// also shrink that box. Only the left side is inset; the leftover room stays on the right.
fn center_pad(field: gpui::Pixels, text_width: gpui::Pixels) -> gpui::Pixels {
    ((field - text_width) * 0.5).max(px(0.))
}

/// Width of `text` in the same font the stepper uses (`text_xs`, 0.75rem).
fn text_width(text: &str, window: &Window) -> gpui::Pixels {
    if text.is_empty() {
        return px(0.);
    }
    let mut style = window.text_style();
    style.font_size = rems(0.75).into();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let run = style.to_run(text.len());
    window
        .text_system()
        .shape_line(text.to_string().into(), font_size, &[run], None)
        .width
}

/// Digits only, with an optional leading minus while `allow_neg` is set.
/// An empty field and a lone `-` are allowed so the user can keep typing.
fn integer_text(text: &str, allow_neg: bool) -> bool {
    let rest = if allow_neg {
        text.strip_prefix('-').unwrap_or(text)
    } else {
        text
    };
    rest.len() <= 8 && rest.bytes().all(|b| b.is_ascii_digit())
}

fn parsed_in_range(text: &str, min: i32, max: i32) -> Option<i32> {
    let value = text.parse::<i32>().ok()?;
    (min..=max).contains(&value).then_some(value)
}

fn commit_value(text: &str, min: i32, max: i32, fallback: i32) -> i32 {
    text.parse::<i32>().unwrap_or(fallback).clamp(min, max)
}

fn step_value(text: &str, current: i32, min: i32, max: i32, step: i32, dir: i32) -> i32 {
    let base = text.parse::<i32>().unwrap_or(current).clamp(min, max);
    if dir < 0 {
        base.saturating_sub(step).max(min)
    } else {
        base.saturating_add(step).min(max)
    }
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
pub fn kv_row(
    key: impl Into<SharedString>,
    value: impl Into<SharedString>,
    odd: bool,
    cx: &App,
) -> gpui::Div {
    h_flex()
        .w_full()
        .px_3()
        .py_1()
        .gap_3()
        .when(odd, |d| d.bg(cx.theme().muted.opacity(0.4)))
        .child(
            div()
                .w(px(200.))
                .flex_shrink_0()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(key.into()),
        )
        .child(div().flex_1().min_w_0().text_sm().child(value.into()))
}

#[cfg(test)]
mod tests {
    use super::{commit_value, integer_text, parsed_in_range, step_value};

    #[test]
    fn integer_text_allows_partial_numbers() {
        assert!(integer_text("", false));
        assert!(integer_text("12", false));
        assert!(integer_text("12345678", false));
        assert!(!integer_text("123456789", false));
        assert!(!integer_text("-3", false));
        assert!(!integer_text("12a", false));
        assert!(integer_text("-", true));
        assert!(integer_text("-3", true));
        assert!(!integer_text("--1", true));
    }

    #[test]
    fn range_parse_and_commit() {
        assert_eq!(parsed_in_range("12", 5, 72), Some(12));
        assert_eq!(parsed_in_range("01", 0, 32), Some(1));
        assert_eq!(parsed_in_range("4", 5, 72), None);
        assert_eq!(parsed_in_range("80", 5, 72), None);
        assert_eq!(parsed_in_range("-", -10, 10), None);
        assert_eq!(commit_value("80", 5, 72, 9), 72);
        assert_eq!(commit_value("3", 5, 72, 9), 5);
        assert_eq!(commit_value("", 5, 72, 9), 9);
        assert_eq!(commit_value("-", -10, 10, 0), 0);
        assert_eq!(commit_value("-8", -10, 10, 0), -8);
    }

    #[test]
    fn center_pad_splits_leftover_space() {
        use gpui::px;
        assert_eq!(super::center_pad(px(44.), px(10.)), px(17.));
        assert_eq!(super::center_pad(px(44.), px(44.)), px(0.));
        assert_eq!(super::center_pad(px(44.), px(80.)), px(0.));
    }

    #[test]
    fn step_clamps_before_moving() {
        assert_eq!(step_value("9", 9, 5, 72, 1, 1), 10);
        assert_eq!(step_value("9", 9, 5, 72, 1, -1), 8);
        assert_eq!(step_value("80", 9, 5, 72, 1, 1), 72);
        assert_eq!(step_value("80", 9, 5, 72, 1, -1), 71);
        assert_eq!(step_value("-", 9, 5, 72, 1, 1), 10);
        assert_eq!(step_value("3", 9, 5, 72, 1, -1), 5);
        assert_eq!(step_value("15", 10, 1, 100, 10, 1), 25);
    }
}
