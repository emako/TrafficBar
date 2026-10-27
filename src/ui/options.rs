//! Options (`COptionsDlg`) with the taskbar window page (`CTaskBarSettingsDlg`) and the
//! general page (`CGeneralSettingsDlg`). The main window page of the original does not exist here.

use gpui::{
    AnyElement, App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, ScrollHandle, StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window,
    div, prelude::FluentBuilder, px, size,
};
use gpui_component::button::Button;
use gpui_component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{IndexPath, Sizable, WindowExt, h_flex, v_flex};

use super::Slot;
use super::common::{
    StepperSet, admin_mark, button, check, choice, colorref_to_hsla, footer, group,
    hsla_to_colorref, label, muted, primary_button, radio, row, stepper, thin_vscroll,
};
use super::display_settings::{ItemOrderModel, hardware_filter};
use crate::config::{
    DoubleClickAction, MONITOR_TIME_SPAN_MAX, MONITOR_TIME_SPAN_MIN, MemoryCleanTrigger,
    MemoryDisplay, Runtime, Settings, SpeedUnit,
};
use crate::display_item::{ALL_BUILTIN, DisplayItem, HardwareItem, ItemKey, all_items};
use crate::i18n::{tr, tr_plain};
use crate::monitor::hardware::HardwareState;
use crate::state;
use crate::taskbar::render::RenderMode;

pub struct OptionsView {
    tab: usize,
    draft: Settings,
    rt: Runtime,
    auto_run: bool,
    auto_run_task: bool,
    orig_auto_run: (bool, bool),
    orig_language: String,
    orig_portable: bool,
    style_modified: bool,
    font_select: Entity<SelectState<SearchableVec<String>>>,
    exe_input: Entity<InputState>,
    steppers: Entity<StepperSet>,
    text_color: Entity<ColorPickerState>,
    back_color: Entity<ColorPickerState>,
    graph_color: Entity<ColorPickerState>,
    item_colors: Vec<(
        ItemKey,
        String,
        Entity<ColorPickerState>,
        Entity<ColorPickerState>,
    )>,
    disp_inputs: Vec<(ItemKey, String, Entity<InputState>)>,
    order_model: Option<ItemOrderModel>,
    adapters: Vec<String>,
    page_scroll: ScrollHandle,
    _subs: Vec<Subscription>,
}

fn item_names() -> Vec<(ItemKey, String)> {
    state::with(|s| {
        let ids = s.plugins.item_ids();
        all_items(&ids)
            .into_iter()
            .map(|k| {
                let name = match &k {
                    ItemKey::Builtin(d) => d.name(),
                    ItemKey::Plugin(id) => s
                        .plugins
                        .item(id)
                        .map(|i| i.name.clone())
                        .unwrap_or_else(|| id.clone()),
                };
                (k, name)
            })
            .collect()
    })
}

impl OptionsView {
    fn new(tab: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (draft, rt) = state::with(|s| (s.settings.clone(), s.rt.clone()));
        let font_name = draft.taskbar.font.name.clone();
        let exe = draft.taskbar.double_click_exe.clone();
        let mut fonts = crate::taskbar::render::system_font_families();
        let selected = fonts
            .iter()
            .position(|name| name.eq_ignore_ascii_case(&font_name))
            .or_else(|| {
                if font_name.is_empty() {
                    None
                } else {
                    fonts.insert(0, font_name);
                    Some(0)
                }
            });
        let font_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(fonts),
                selected.map(IndexPath::new),
                window,
                cx,
            )
            .searchable(true)
        });
        let exe_input = cx.new(|cx| InputState::new(window, cx).default_value(exe));
        let steppers = cx.new(|_cx| StepperSet::default());
        let first_label = draft
            .taskbar
            .text_colors
            .get(&ItemKey::Builtin(DisplayItem::Up))
            .map(|c| c.label)
            .unwrap_or(0xFFFFFF);
        let text_color = cx.new(|cx| {
            ColorPickerState::new(window, cx).default_value(colorref_to_hsla(first_label))
        });
        let back_color = cx.new(|cx| {
            ColorPickerState::new(window, cx)
                .default_value(colorref_to_hsla(draft.taskbar.back_color))
        });
        let graph_color = cx.new(|cx| {
            ColorPickerState::new(window, cx)
                .default_value(colorref_to_hsla(draft.taskbar.status_bar_color))
        });

        let mut subs = Vec::new();
        subs.push(cx.subscribe(
            &font_select,
            |this: &mut Self, _, ev: &SelectEvent<SearchableVec<String>>, cx| {
                if let SelectEvent::Confirm(Some(name)) = ev {
                    this.draft.taskbar.font.name = name.clone();
                    cx.notify();
                }
            },
        ));
        subs.push(
            cx.subscribe(&exe_input, |this: &mut Self, input, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.draft.taskbar.double_click_exe = input.read(cx).value().to_string();
                }
            }),
        );
        subs.push(cx.subscribe(
            &text_color,
            |this: &mut Self, _, ev: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(Some(c)) = ev else {
                    return;
                };
                let c = hsla_to_colorref(*c);
                if let Some(item) = this
                    .draft
                    .taskbar
                    .text_colors
                    .get_mut(&ItemKey::Builtin(DisplayItem::Up))
                {
                    item.label = c;
                }
                this.style_modified = true;
                cx.notify();
            },
        ));
        subs.push(cx.subscribe(
            &back_color,
            |this: &mut Self, _, ev: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(Some(c)) = ev else {
                    return;
                };
                let transparent = this.draft.taskbar.is_transparent(&this.rt);
                this.draft.taskbar.back_color = hsla_to_colorref(*c);
                if transparent {
                    crate::config::transparent_color_convert(&mut this.draft.taskbar.back_color);
                    this.draft.taskbar.transparent_color = this.draft.taskbar.back_color;
                }
                this.style_modified = true;
                cx.notify();
            },
        ));
        subs.push(cx.subscribe(
            &graph_color,
            |this: &mut Self, _, ev: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(Some(c)) = ev else {
                    return;
                };
                this.draft.taskbar.status_bar_color = hsla_to_colorref(*c);
                this.draft.taskbar.graph_color_following_system = false;
                this.style_modified = true;
                cx.notify();
            },
        ));

        let auto_run_task = draft.general.auto_run_by_task_scheduler;
        let auto_run = draft.general.auto_run;
        OptionsView {
            tab,
            orig_language: draft.general.language.clone(),
            orig_portable: draft.general.portable_mode,
            draft,
            rt,
            auto_run,
            auto_run_task,
            orig_auto_run: (auto_run, auto_run_task),
            style_modified: false,
            font_select,
            exe_input,
            steppers,
            text_color,
            back_color,
            graph_color,
            item_colors: Vec::new(),
            disp_inputs: Vec::new(),
            order_model: None,
            adapters: Vec::new(),
            page_scroll: ScrollHandle::new(),
            _subs: subs,
        }
    }

    fn reset_color_pickers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let t = &self.draft.taskbar;
        let first = t
            .text_colors
            .get(&ItemKey::Builtin(DisplayItem::Up))
            .map(|c| c.label)
            .unwrap_or(0);
        let (back, graph) = (t.back_color, t.status_bar_color);
        self.text_color
            .update(cx, |s, cx| s.set_value(colorref_to_hsla(first), window, cx));
        self.back_color
            .update(cx, |s, cx| s.set_value(colorref_to_hsla(back), window, cx));
        self.graph_color
            .update(cx, |s, cx| s.set_value(colorref_to_hsla(graph), window, cx));
    }

    /// `COptionsDlg::OnOK` / apply.
    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut draft = self.draft.clone();
        draft.taskbar.valid();
        if draft.taskbar.auto_save_taskbar_color_settings_to_preset
            && draft.taskbar.auto_adapt_light_theme
            && self.style_modified
        {
            let idx = if self.rt.light_theme {
                draft.taskbar.light_default_style
            } else {
                draft.taskbar.dark_default_style
            };
            draft.modify_default_style(idx);
        }
        self.style_modified = false;

        if (self.auto_run, self.auto_run_task) != self.orig_auto_run {
            let result = if !self.auto_run && self.orig_auto_run.0 {
                crate::app::set_auto_run(false, self.orig_auto_run.1)
            } else {
                crate::app::set_auto_run(self.auto_run, self.auto_run_task)
            };
            if let Err(e) = result {
                crate::app::post(crate::app::MainMsg::MessageBox {
                    text: e,
                    warning: true,
                });
            }
            self.orig_auto_run = (self.auto_run, self.auto_run_task);
        }
        draft.general.auto_run = self.auto_run;
        draft.general.auto_run_by_task_scheduler = self.auto_run_task;
        draft.general.memory_clean_mask =
            crate::memory::normalize_mask(draft.general.memory_clean_mask);
        draft.general.memory_clean_threshold = draft.general.memory_clean_threshold.clamp(1, 100);

        if !crate::win::is_portable_edition() {
            draft.general.portable_mode = false;
        }
        let mut notes = Vec::new();
        if draft.general.portable_mode != self.orig_portable {
            crate::config::save_global_config(draft.general.portable_mode);
            self.orig_portable = draft.general.portable_mode;
            notes.push(tr("IDS_CFG_DIR_CHANGED_INFO"));
        }
        if draft.general.language != self.orig_language {
            self.orig_language = draft.general.language.clone();
            notes.push(tr("IDS_LANGUAGE_CHANGE_INFO"));
        }
        self.draft = draft.clone();
        crate::app::apply_settings(draft);
        if !notes.is_empty() {
            let text = notes.join("\n");
            window.open_dialog(cx, move |dialog, _, _| {
                dialog.alert().title(tr("APP_NAME")).child(text.clone())
            });
        }
    }

    fn update<F: FnOnce(&mut Self, &mut Window, &mut Context<Self>)>(
        this: &WeakEntity<Self>,
        window: &mut Window,
        cx: &mut App,
        f: F,
    ) {
        if let Some(v) = this.upgrade() {
            v.update(cx, |v, cx| {
                f(v, window, cx);
                cx.notify();
            });
        }
    }

    // ----------------------------------------------------------------- taskbar page

    fn color_font_group(
        &self,
        me: &WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = &self.draft.taskbar;
        let rt = self.rt.clone();
        let m = me.clone();
        let font = &t.font;
        let font_row = row()
            .child(label(tr("TXT_FONT")))
            .child(
                div()
                    .w(px(180.))
                    .child(Select::new(&self.font_select).small().menu_width(px(240.))),
            )
            .child(label(tr("TXT_FONT_SIZE")))
            .child(stepper(
                &self.steppers,
                "font-size",
                font.size,
                5,
                72,
                1,
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| Self::update(&m, w, cx, |s, _, _| s.draft.taskbar.font.size = v)
                },
                window,
                cx,
            ))
            .child(check(
                "font-bold",
                tr("TXT_FONT_BOLD"),
                font.bold,
                false,
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| Self::update(&m, w, cx, |s, _, _| s.draft.taskbar.font.bold = v)
                },
                cx,
            ))
            .child(check(
                "font-italic",
                tr("TXT_FONT_ITALIC"),
                font.italic,
                false,
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| {
                        Self::update(&m, w, cx, |s, _, _| s.draft.taskbar.font.italic = v)
                    }
                },
                cx,
            ))
            .child(check(
                "font-underline",
                tr("TXT_FONT_UNDERLINE"),
                font.underline,
                false,
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| {
                        Self::update(&m, w, cx, |s, _, _| s.draft.taskbar.font.underline = v)
                    }
                },
                cx,
            ))
            .child(check(
                "font-strike",
                tr("TXT_FONT_STRIKEOUT"),
                font.strike_out,
                false,
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| {
                        Self::update(&m, w, cx, |s, _, _| s.draft.taskbar.font.strike_out = v)
                    }
                },
                cx,
            ));

        let text_color_ctrl: AnyElement = if t.specify_each_item_color {
            let m = m.clone();
            button(
                "item-colors",
                tr_plain("TXT_TITLE_TASKBAR_COLOR_DLG") + "...",
                false,
                move |w, cx| {
                    Self::update(&m, w, cx, |s, w, cx| s.open_item_colors(w, cx));
                },
            )
        } else {
            ColorPicker::new(&self.text_color)
                .small()
                .into_any_element()
        };

        let presets: Vec<String> = (0..crate::config::TASKBAR_DEFAULT_STYLE_NUM)
            .map(|i| {
                let mut s = format!("{} {}", tr("IDS_PRESET"), i + 1);
                if i == 2 || i == 3 {
                    s += &format!(" ({})", tr("IDS_LIGHT_MODE"));
                }
                s
            })
            .collect();
        let apply_presets = presets.clone();
        let m_apply = m.clone();
        let preset_btn = Button::new("preset")
            .label(tr_plain("TXT_PRESET"))
            .small()
            .outline()
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for (i, p) in apply_presets.iter().enumerate() {
                    let m = m_apply.clone();
                    menu = menu.item(PopupMenuItem::new(p.clone()).on_click(move |_, w, cx| {
                        Self::update(&m, w, cx, |s, w, cx| {
                            s.draft.apply_default_style(i as i32);
                            s.style_modified = true;
                            s.reset_color_pickers(w, cx);
                        })
                    }));
                }
                menu
            });
        let m_mod = m.clone();
        let modify_btn = Button::new("modify-preset")
            .label(tr("IDS_MODIFY_PRESET"))
            .small()
            .outline()
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for (i, p) in presets.iter().enumerate() {
                    let m = m_mod.clone();
                    menu = menu.item(PopupMenuItem::new(p.clone()).on_click(move |_, w, cx| {
                        let m = m.clone();
                        let text = tr("IDS_SAVE_DEFAULT_STYLE_INQUIRY")
                            .replace("<%1%>", &(i + 1).to_string());
                        w.open_dialog(cx, move |dialog, _, _| {
                            let m = m.clone();
                            dialog
                                .confirm()
                                .title(tr("IDS_MODIFY_PRESET"))
                                .child(text.clone())
                                .on_ok(move |_, w, cx| {
                                    Self::update(&m, w, cx, |s, _, _| {
                                        s.draft.modify_default_style(i as i32)
                                    });
                                    true
                                })
                        });
                    }));
                }
                menu
            });

        let transparent = t.is_transparent(&rt);
        group(tr("TXT_COLOR_AND_FONT"), cx)
            .child(font_row)
            .child(
                row()
                    .child(label(tr("TXT_TEXT_COLOR")))
                    .child(text_color_ctrl)
                    .child(check(
                        "specify-each",
                        tr("TXT_SPECIFY_EACH_ITEM_COLOR_CHECK"),
                        t.specify_each_item_color,
                        false,
                        false,
                        {
                            let m = m.clone();
                            move |v, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.taskbar.specify_each_item_color = v;
                                    s.style_modified = true;
                                })
                            }
                        },
                        cx,
                    ))
                    .child(preset_btn)
                    .child(modify_btn),
            )
            .child(
                row()
                    .child(label(tr("TXT_BACKGROUND_COLOR")))
                    .child(ColorPicker::new(&self.back_color).small())
                    .child(check(
                        "bg-transparent",
                        tr("TXT_BACKGROUND_TRANSPARENT"),
                        transparent,
                        false,
                        false,
                        {
                            let m = m.clone();
                            move |v, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    let rt = s.rt.clone();
                                    s.draft.taskbar.set_transparent(v, &rt);
                                    s.style_modified = true;
                                })
                            }
                        },
                        cx,
                    )),
            )
            .child(
                row()
                    .child(check(
                        "auto-adapt",
                        tr("TXT_AUTO_ADAPT_LIGHT_THEME"),
                        t.auto_adapt_light_theme,
                        !rt.version.is_windows10_or_later(),
                        false,
                        {
                            let m = m.clone();
                            move |v, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.taskbar.auto_adapt_light_theme = v
                                })
                            }
                        },
                        cx,
                    ))
                    .child(button(
                        "auto-adapt-settings",
                        tr_plain("TXT_AUTO_ADAPT_SETTINGS_BUTTON"),
                        !t.auto_adapt_light_theme,
                        {
                            let m = m.clone();
                            move |w, cx| {
                                Self::update(&m, w, cx, |s, w, cx| s.open_auto_adapt(w, cx))
                            }
                        },
                    )),
            )
            .into_any_element()
    }

    fn display_group(
        &self,
        me: &WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = &self.draft.taskbar;
        let m = me.clone();
        let memory_options = vec![
            tr("IDS_USAGE_PERCENTAGE"),
            tr("IDS_MEMORY_USED"),
            tr("IDS_MEMORY_AVAILABLE"),
        ];
        let digits: Vec<String> = (3..=7).map(|d| d.to_string()).collect();
        macro_rules! toggle {
            ($id:expr, $key:expr, $field:ident) => {
                check(
                    $id,
                    tr($key),
                    t.$field,
                    false,
                    false,
                    {
                        let m = m.clone();
                        move |v, w, cx| {
                            Self::update(&m, w, cx, |s, _, _| s.draft.taskbar.$field = v)
                        }
                    },
                    cx,
                )
            };
        }
        group(tr("TXT_DISPLAY_SETTINGS"), cx)
            .flex_1()
            .min_w(px(0.))
            .child(
                row()
                    .child(button(
                        "display-text",
                        tr_plain("TXT_DISPLAY_TEXT_SETTING_BUTTON"),
                        false,
                        {
                            let m = m.clone();
                            move |w, cx| {
                                Self::update(&m, w, cx, |s, w, cx| s.open_display_text(w, cx))
                            }
                        },
                    ))
                    .child(button(
                        "display-order",
                        tr_plain("TXT_DISPLAY_SETTINGS_BUTTON"),
                        false,
                        {
                            let m = m.clone();
                            move |w, cx| {
                                Self::update(&m, w, cx, |s, w, cx| s.open_item_order(w, cx))
                            }
                        },
                    )),
            )
            .child(toggle!(
                "short-mode",
                "TXT_SPEED_SHORT_MODE",
                speed_short_mode
            ))
            .child(toggle!(
                "right-align",
                "TXT_VALUE_RIGHT_ALIGN",
                value_right_align
            ))
            .child(
                row()
                    .child(label(tr("TXT_NET_SPEED_DATA_WIDTH")))
                    .child(choice(
                        "digits",
                        digits,
                        (t.digits_number - 3).clamp(0, 4) as usize,
                        false,
                        {
                            let m = m.clone();
                            move |i, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.taskbar.digits_number = i as i32 + 3
                                })
                            }
                        },
                    ))
                    .child(label(tr("TXT_CHARACTORS"))),
            )
            .child(toggle!(
                "horizontal",
                "TXT_HORIZONTAL_ARRANGE",
                horizontal_arrange
            ))
            .child(toggle!(
                "separate-unit",
                "TXT_SEPARATE_VALUE_UNIT_CHECK",
                separate_value_unit_with_space
            ))
            .child(toggle!("tooltip", "TXT_SHOW_TOOL_TIP_CHK", show_tool_tip))
            .child(
                row()
                    .child(label(tr("TXT_MEMORY_DISPLAY_MODE")))
                    .child(choice(
                        "memory-display",
                        memory_options,
                        t.memory_display as usize,
                        false,
                        {
                            let m = m.clone();
                            move |i, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.taskbar.memory_display =
                                        MemoryDisplay::from_i32(i as i32)
                                })
                            }
                        },
                    )),
            )
            .child(
                row()
                    .child(label(tr("TXT_ITEM_SPACING")))
                    .child(stepper(
                        &self.steppers,
                        "item-space",
                        t.item_space,
                        0,
                        32,
                        1,
                        false,
                        {
                            let m = m.clone();
                            move |v, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| s.draft.taskbar.item_space = v)
                            }
                        },
                        window,
                        cx,
                    ))
                    .child(label(tr("TXT_PIXELS")))
                    .child(label(tr("TXT_VERTICAL_MARGIN")))
                    .child(stepper(
                        &self.steppers,
                        "vertical-margin",
                        t.vertical_margin,
                        -10,
                        10,
                        1,
                        t.horizontal_arrange,
                        {
                            let m = m.clone();
                            move |v, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.taskbar.vertical_margin = v
                                })
                            }
                        },
                        window,
                        cx,
                    ))
                    .child(label(tr("TXT_PIXELS"))),
            )
            .into_any_element()
    }

    fn unit_group(
        &self,
        me: &WeakEntity<Self>,
        cx: &mut Context<Self>,
        width: gpui::Pixels,
    ) -> AnyElement {
        let t = &self.draft.taskbar;
        let m = me.clone();
        let unit_suffix = if t.unit_byte { "B/s" } else { "b/s" };
        let units = vec![
            tr("IDS_AUTO"),
            format!("{} K{}", tr("IDS_FIXED_AS"), unit_suffix),
            format!("{} M{}", tr("IDS_FIXED_AS"), unit_suffix),
        ];
        group(tr("TXT_UNIT_SETTINGS"), cx)
            .w(width)
            .min_w(width)
            .max_w(width)
            .flex_shrink_0()
            .child(
                row()
                    .child(label(tr("TXT_NETSPEED_UNIT")))
                    .child(radio(
                        "unit-byte",
                        tr("TXT_UNIT_BYTE_RADIO"),
                        t.unit_byte,
                        false,
                        {
                            let m = m.clone();
                            move |w, cx| {
                                Self::update(&m, w, cx, |s, _, _| s.draft.taskbar.unit_byte = true)
                            }
                        },
                    ))
                    .child(radio(
                        "unit-bit",
                        tr("TXT_UNIT_BIT_RADIO"),
                        !t.unit_byte,
                        false,
                        {
                            let m = m.clone();
                            move |w, cx| {
                                Self::update(&m, w, cx, |s, _, _| s.draft.taskbar.unit_byte = false)
                            }
                        },
                    )),
            )
            .child(row().child(label(tr("TXT_UNIT_SELECTION"))).child(choice(
                "speed-unit",
                units,
                t.speed_unit as usize,
                false,
                {
                    let m = m.clone();
                    move |i, w, cx| {
                        Self::update(&m, w, cx, |s, _, _| {
                            s.draft.taskbar.speed_unit = SpeedUnit::from_i32(i as i32)
                        })
                    }
                },
            )))
            .child(check(
                "hide-unit",
                tr("TXT_HIDE_UNIT_CHECK"),
                t.hide_unit,
                t.speed_unit == SpeedUnit::Auto,
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| Self::update(&m, w, cx, |s, _, _| s.draft.taskbar.hide_unit = v)
                },
                cx,
            ))
            .child(check(
                "hide-percent",
                tr("TXT_HIDE_PERCENTAGE_CHECK"),
                t.hide_percent,
                false,
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| {
                        Self::update(&m, w, cx, |s, _, _| s.draft.taskbar.hide_percent = v)
                    }
                },
                cx,
            ))
            .into_any_element()
    }

    fn taskbar_wnd_group(&self, me: &WeakEntity<Self>, cx: &mut Context<Self>) -> AnyElement {
        let t = &self.draft.taskbar;
        let m = me.clone();
        let mut displays = vec![tr("IDS_PRIMARY_DISPLAY")];
        let secondary = crate::taskbar::secondary_taskbars()
            .len()
            .max(crate::taskbar::display_count().saturating_sub(1));
        for i in 0..secondary {
            displays.push(tr("IDS_SECONDARY_DISPLAY").replace("<%1%>", &(i + 1).to_string()));
        }
        let display_index = if t.show_taskbar_wnd_in_secondary_display {
            (t.secondary_display_index + 1).max(1) as usize
        } else {
            0
        };
        let actions: Vec<String> = DoubleClickAction::ALL.iter().map(|a| a.label()).collect();
        let action_index = DoubleClickAction::ALL
            .iter()
            .position(|a| *a == t.double_click_action)
            .unwrap_or(0);
        let on_left_enabled = !self.rt.win11_taskbar || crate::win::is_taskbar_center_align();
        group(tr("TXT_TASKBAR_WINDOW"), cx)
            .child(check(
                "wnd-on-left",
                tr("TXT_TASKBAR_WND_ON_LEFT"),
                t.tbar_wnd_on_left,
                !on_left_enabled,
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| {
                        Self::update(&m, w, cx, |s, _, _| s.draft.taskbar.tbar_wnd_on_left = v)
                    }
                },
                cx,
            ))
            .child(
                row()
                    .child(label(tr("TXT_DISPLAY_TO_SHOW_TASKBAR_WND")))
                    .child(choice(
                        "display",
                        displays,
                        display_index,
                        secondary == 0,
                        {
                            let m = m.clone();
                            move |i, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.taskbar.show_taskbar_wnd_in_secondary_display = i > 0;
                                    s.draft.taskbar.secondary_display_index =
                                        i.saturating_sub(1) as i32;
                                })
                            }
                        },
                    )),
            )
            .child(row().child(button(
                "win11-settings",
                tr("TXT_WIN11_SETTINGS_BUTTON"),
                !self.rt.win11_taskbar,
                {
                    let m = m.clone();
                    move |w, cx| Self::update(&m, w, cx, |s, w, cx| s.open_win11(w, cx))
                },
            )))
            .child(
                row()
                    .child(label(tr("TXT_DOUBLE_CLICK_ACTION")))
                    .child(choice("double-click", actions, action_index, false, {
                        let m = m.clone();
                        move |i, w, cx| {
                            Self::update(&m, w, cx, |s, _, _| {
                                s.draft.taskbar.double_click_action = DoubleClickAction::ALL[i]
                            })
                        }
                    })),
            )
            .when(
                t.double_click_action == DoubleClickAction::SpecificApp,
                |g| {
                    g.child(
                        row()
                            .child(label(tr("TXT_EXE_PATH_STATIC")))
                            .child(div().w(px(300.)).child(Input::new(&self.exe_input).small())),
                    )
                },
            )
            .into_any_element()
    }

    fn graph_group(
        &self,
        me: &WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = &self.draft.taskbar;
        let m = me.clone();
        let any_graph = t.show_status_bar || t.show_netspeed_figure;
        group(tr("TXT_RESOURCE_USAGE_GRAPH"), cx)
            .child(
                row()
                    .child(check(
                        "show-status-bar",
                        tr("TXT_SHOW_RESOURCE_USAGE_GRAPH"),
                        t.show_status_bar,
                        false,
                        false,
                        {
                            let m = m.clone();
                            move |v, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.taskbar.show_status_bar = v
                                })
                            }
                        },
                        cx,
                    ))
                    .child(check(
                        "dashed-box",
                        tr("TXT_SHOW_DASHED_BOX"),
                        t.show_graph_dashed_box,
                        !any_graph,
                        false,
                        {
                            let m = m.clone();
                            move |v, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.taskbar.show_graph_dashed_box = v
                                })
                            }
                        },
                        cx,
                    )),
            )
            .child(check(
                "netspeed-figure",
                tr("TXT_SHOW_NET_SPEED_GRAPH"),
                t.show_netspeed_figure,
                false,
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| {
                        Self::update(&m, w, cx, |s, _, _| {
                            s.draft.taskbar.show_netspeed_figure = v
                        })
                    }
                },
                cx,
            ))
            .child(
                row()
                    .child(label(tr("TXT_NET_SPEED_GRAPH_MAX_VALUE")))
                    .child(stepper(
                        &self.steppers,
                        "netspeed-max",
                        t.netspeed_figure_max_value,
                        1,
                        1024,
                        1,
                        !t.show_netspeed_figure,
                        {
                            let m = m.clone();
                            move |v, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.taskbar.netspeed_figure_max_value = v
                                })
                            }
                        },
                        window,
                        cx,
                    ))
                    .child(choice(
                        "netspeed-max-unit",
                        vec!["KB".into(), "MB".into()],
                        t.netspeed_figure_max_value_unit.clamp(0, 1) as usize,
                        !t.show_netspeed_figure,
                        {
                            let m = m.clone();
                            move |i, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.taskbar.netspeed_figure_max_value_unit = i as i32
                                })
                            }
                        },
                    )),
            )
            .child(
                row()
                    .child(label(tr("TXT_USAGE_GRAPH_COLOR")))
                    .child(ColorPicker::new(&self.graph_color).small())
                    .child(check(
                        "graph-follow",
                        tr("TXT_USAGE_GRAPH_FOLLOW_SYSTEM_CHECK"),
                        t.graph_color_following_system,
                        !any_graph,
                        false,
                        {
                            let m = m.clone();
                            move |v, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.taskbar.graph_color_following_system = v
                                })
                            }
                        },
                        cx,
                    )),
            )
            .child(
                row()
                    .child(label(tr("TXT_GRAPH_DISPLAY_MODE")))
                    .child(radio(
                        "graph-bar",
                        tr("TXT_BAR_MODE"),
                        !t.cm_graph_type,
                        !any_graph,
                        {
                            let m = m.clone();
                            move |w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.taskbar.cm_graph_type = false
                                })
                            }
                        },
                    ))
                    .child(radio(
                        "graph-plot",
                        tr("TXT_PLOT_MODE"),
                        t.cm_graph_type,
                        !any_graph,
                        {
                            let m = m.clone();
                            move |w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.taskbar.cm_graph_type = true
                                })
                            }
                        },
                    )),
            )
            .into_any_element()
    }

    fn render_group(&self, me: &WeakEntity<Self>, cx: &mut Context<Self>) -> AnyElement {
        let t = &self.draft.taskbar;
        let m = me.clone();
        let mode = state::with(|s| s.render_mode);
        let mode_text = match mode {
            RenderMode::Layered => tr("BAR_RENDER_ULW"),
            RenderMode::Composition => tr("BAR_RENDER_DCOMP"),
        };
        group(tr("TXT_RENDERING_SETTINGS"), cx)
            .child(label(format!("{}{}", tr("BAR_RENDER_MODE"), mode_text)))
            .child(check(
                "color-emoji",
                tr("TXT_ENABLE_COLOR_EMOJI"),
                t.enable_colorful_emoji,
                false,
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| {
                        Self::update(&m, w, cx, |s, _, _| {
                            s.draft.taskbar.enable_colorful_emoji = v
                        })
                    }
                },
                cx,
            ))
            .into_any_element()
    }

    // ----------------------------------------------------------------- general page

    fn app_group(&self, me: &WeakEntity<Self>, cx: &mut Context<Self>) -> AnyElement {
        let g = &self.draft.general;
        let m = me.clone();
        let language_ids: Vec<String> = crate::i18n::choices()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let mut languages = vec![tr("BAR_FOLLOW_SYSTEM")];
        languages.extend(crate::i18n::choices().into_iter().map(|(_, name)| name));
        let lang_index = crate::i18n::choice_index(&g.language);
        let checking = state::with(|s| s.checking_update);
        group(tr("TXT_APPLICATION_SETTINGS"), cx)
            .flex_1()
            .min_w(px(0.))
            .child(
                row()
                    .child(check(
                        "check-update",
                        tr("TXT_CHECK_UPDATE"),
                        g.check_update_when_start,
                        false,
                        false,
                        {
                            let m = m.clone();
                            move |v, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.general.check_update_when_start = v
                                })
                            }
                        },
                        cx,
                    ))
                    .child(button(
                        "check-now",
                        tr_plain("TXT_CHECK_NOW"),
                        checking,
                        |_, cx| super::update::check(false, cx),
                    )),
            )
            .child(
                row()
                    .child(label(tr("TXT_UPDATE_SOURCE")))
                    .child(radio(
                        "src-github",
                        "GitHub",
                        g.update_source == 0,
                        false,
                        {
                            let m = m.clone();
                            move |w, cx| {
                                Self::update(&m, w, cx, |s, _, _| s.draft.general.update_source = 0)
                            }
                        },
                    ))
                    .child(radio("src-gitee", "Gitee", g.update_source == 1, false, {
                        let m = m.clone();
                        move |w, cx| {
                            Self::update(&m, w, cx, |s, _, _| s.draft.general.update_source = 1)
                        }
                    })),
            )
            .child(row().child(label(tr("TXT_LANGUAGE"))).child(choice(
                "language",
                languages,
                lang_index,
                false,
                {
                    let m = m.clone();
                    let language_ids = language_ids.clone();
                    move |i, w, cx| {
                        Self::update(&m, w, cx, |s, _, _| {
                            s.draft.general.language = if i == 0 {
                                String::new()
                            } else {
                                language_ids.get(i - 1).cloned().unwrap_or_default()
                            };
                        })
                    }
                },
            )))
            .child(check(
                "always-admin",
                tr("TXT_ALWAYS_RUN_AS_ADMIN"),
                g.always_run_as_admin,
                false,
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| {
                        Self::update(&m, w, cx, |s, _, _| s.draft.general.always_run_as_admin = v)
                    }
                },
                cx,
            ))
            .child(check(
                "efficiency-mode",
                tr("TXT_EFFICIENCY_MODE"),
                g.efficiency_mode,
                !self.rt.version.is_windows11_or_later(),
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| {
                        Self::update(&m, w, cx, |s, _, _| s.draft.general.efficiency_mode = v)
                    }
                },
                cx,
            ))
            .child(
                muted(tr("TXT_EFFICIENCY_MODE_TIP"), cx)
                    .w_full()
                    .whitespace_normal(),
            )
            .into_any_element()
    }

    fn data_group(
        &self,
        me: &WeakEntity<Self>,
        cx: &mut Context<Self>,
        width: gpui::Pixels,
    ) -> AnyElement {
        let g = &self.draft.general;
        let m = me.clone();
        let writable = state::with(|s| s.exe_dir_writable);
        let portable_edition = crate::win::is_portable_edition();
        let dir = self.draft.config_dir.clone();
        group(tr("TXT_CONFIGURATION_AND_DATA_FILES"), cx)
            .w(width)
            .min_w(width)
            .max_w(width)
            .flex_shrink_0()
            .child(radio(
                "save-appdata",
                tr("TXT_SAVE_TO_APPDATA_RADIO"),
                !g.portable_mode || !portable_edition,
                false,
                {
                    let m = m.clone();
                    move |w, cx| {
                        Self::update(&m, w, cx, |s, _, _| s.draft.general.portable_mode = false)
                    }
                },
            ))
            .child(radio(
                "save-exe-dir",
                tr("TXT_SAVE_TO_PROGRAM_DIR_RADIO"),
                g.portable_mode && portable_edition,
                !writable || !portable_edition,
                {
                    let m = m.clone();
                    move |w, cx| {
                        Self::update(&m, w, cx, |s, _, _| {
                            s.draft.general.portable_mode = crate::win::is_portable_edition()
                        })
                    }
                },
            ))
            .child(row().child(button(
                "open-config-dir",
                tr_plain("TXT_OPEN_CONFIG_PATH_BUTTON"),
                false,
                move |_, _| {
                    crate::win::shell_open(&dir.to_string_lossy());
                },
            )))
            .into_any_element()
    }

    /// `IDC_SHOW_NOTIFY_ICON_CHECK` inside the original "显示" group.
    fn notify_icon_group(
        &self,
        me: &WeakEntity<Self>,
        cx: &mut Context<Self>,
        width: gpui::Pixels,
    ) -> AnyElement {
        let show = self.draft.general.show_notify_icon;
        let m = me.clone();
        group(tr("TXT_DISPLAY"), cx)
            .w(width)
            .min_w(width)
            .max_w(width)
            .flex_shrink_0()
            .child(check(
                "show-notify-icon",
                tr_plain("TXT_SHOW_NOTIFY_ICON_CHECK"),
                show,
                false,
                false,
                move |v, w, cx| {
                    Self::update(&m, w, cx, |s, _, _| s.draft.general.show_notify_icon = v)
                },
                cx,
            ))
            .into_any_element()
    }

    fn auto_run_group(&self, me: &WeakEntity<Self>, cx: &mut Context<Self>) -> AnyElement {
        let m = me.clone();
        let elevated = self.rt.elevated;
        group(tr("TXT_AUTO_RUN"), cx)
            .child(check(
                "auto-run",
                tr("TXT_AUTO_RUN_CHECK"),
                self.auto_run,
                false,
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| Self::update(&m, w, cx, |s, _, _| s.auto_run = v)
                },
                cx,
            ))
            .child(
                row()
                    .child(label(tr("TXT_AUTO_RUN_METHOD")))
                    .child(radio(
                        "run-registry",
                        tr("TXT_AUTO_RUN_METHOD_REGESTRY"),
                        !self.auto_run_task,
                        false,
                        {
                            let m = m.clone();
                            move |w, cx| Self::update(&m, w, cx, |s, _, _| s.auto_run_task = false)
                        },
                    ))
                    .child(radio(
                        "run-task",
                        tr("TXT_AUTO_RUN_METHOD_TASK_SCHEDULE"),
                        self.auto_run_task,
                        !elevated,
                        {
                            let m = m.clone();
                            move |w, cx| Self::update(&m, w, cx, |s, _, _| s.auto_run_task = true)
                        },
                    ))
                    .when(!elevated, |r| r.child(admin_mark("run-task-admin", cx))),
            )
            .into_any_element()
    }

    fn memory_area(
        &self,
        me: &WeakEntity<Self>,
        id: &'static str,
        bit: u32,
        key: &'static str,
        cx: &App,
    ) -> AnyElement {
        let on = self.draft.general.memory_clean_mask & bit != 0;
        let m = me.clone();
        check(
            id,
            tr(key),
            on,
            false,
            false,
            move |v, w, cx| {
                Self::update(&m, w, cx, |s, _, _| {
                    let mask = &mut s.draft.general.memory_clean_mask;
                    if v {
                        *mask |= bit;
                    } else {
                        *mask &= !bit;
                    }
                })
            },
            cx,
        )
    }

    /// Memory cleaning. Sits between autostart and notifications on the general page.
    fn memory_clean_group(
        &self,
        me: &WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let g = &self.draft.general;
        let m = me.clone();
        let trigger = g.memory_clean_trigger;
        let threshold = trigger == MemoryCleanTrigger::Threshold;
        let areas = [
            (
                "mc-working-set",
                crate::memory::WORKING_SET,
                "TXT_MEMORY_CLEAN_WORKING_SET",
            ),
            (
                "mc-system-cache",
                crate::memory::SYSTEM_FILE_CACHE,
                "TXT_MEMORY_CLEAN_SYSTEM_FILE_CACHE",
            ),
            (
                "mc-modified-cache",
                crate::memory::MODIFIED_FILE_CACHE,
                "TXT_MEMORY_CLEAN_MODIFIED_FILE_CACHE",
            ),
            (
                "mc-standby0",
                crate::memory::STANDBY_PRIORITY0,
                "TXT_MEMORY_CLEAN_STANDBY_PRIORITY0",
            ),
            (
                "mc-registry",
                crate::memory::REGISTRY_CACHE,
                "TXT_MEMORY_CLEAN_REGISTRY",
            ),
            (
                "mc-combine",
                crate::memory::COMBINE_MEMORY_LISTS,
                "TXT_MEMORY_CLEAN_COMBINE",
            ),
            (
                "mc-standby",
                crate::memory::STANDBY_LIST,
                "TXT_MEMORY_CLEAN_STANDBY",
            ),
            (
                "mc-modified-list",
                crate::memory::MODIFIED_PAGE_LIST,
                "TXT_MEMORY_CLEAN_MODIFIED_LIST",
            ),
        ];
        let mut left = v_flex().gap_2().flex_1().min_w_0();
        let mut right = v_flex().gap_2().flex_1().min_w_0();
        for (i, (id, bit, key)) in areas.into_iter().enumerate() {
            let item = self.memory_area(me, id, bit, key, cx);
            if i % 2 == 0 {
                left = left.child(item);
            } else {
                right = right.child(item);
            }
        }
        group(tr("TXT_RELEASE_MEMORY"), cx)
            .child(
                row()
                    .child(label(tr("TXT_MEMORY_CLEAN_TRIGGER")))
                    .child(choice(
                        "memory-clean-trigger",
                        vec![
                            tr("TXT_MEMORY_CLEAN_DOUBLE_CLICK"),
                            tr("TXT_MEMORY_CLEAN_SINGLE_CLICK"),
                            tr("TXT_MEMORY_CLEAN_THRESHOLD"),
                            tr("IDS_NONE"),
                        ],
                        trigger as usize,
                        false,
                        {
                            let m = m.clone();
                            move |i, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.general.memory_clean_trigger = match i {
                                        1 => MemoryCleanTrigger::Click,
                                        2 => MemoryCleanTrigger::Threshold,
                                        3 => MemoryCleanTrigger::None,
                                        _ => MemoryCleanTrigger::DoubleClick,
                                    }
                                })
                            }
                        },
                    ))
                    .when(threshold, |r| {
                        let m1 = m.clone();
                        r.child(stepper(
                            &self.steppers,
                            "memory-clean-threshold",
                            g.memory_clean_threshold,
                            1,
                            100,
                            1,
                            false,
                            move |v, w, cx| {
                                Self::update(&m1, w, cx, |s, _, _| {
                                    s.draft.general.memory_clean_threshold = v
                                })
                            },
                            window,
                            cx,
                        ))
                        .child(label(tr("TXT_MEMORY_CLEAN_THRESHOLD_SUFFIX")))
                    })
                    .when(!self.rt.elevated, |r| r.child(admin_mark("memory-clean-admin", cx))),
            )
            .when(threshold, |g| {
                g.child(muted(tr("TXT_MEMORY_CLEAN_INTERVAL_HINT"), cx))
            })
            .child(label(tr("TXT_MEMORY_CLEAN_AREAS")))
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_start()
                    .child(left)
                    .child(right),
            )
            .child(muted(tr("TXT_MEMORY_CLEAN_FREEZE_HINT"), cx))
            .child(check(
                "memory-clean-notify",
                tr("TXT_MEMORY_CLEAN_NOTIFY"),
                g.memory_clean_notify,
                false,
                false,
                move |v, w, cx| {
                    Self::update(&m, w, cx, |s, _, _| s.draft.general.memory_clean_notify = v)
                },
                cx,
            ))
            .into_any_element()
    }

    fn notify_group(
        &self,
        me: &WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let g = &self.draft.general;
        let m = me.clone();
        let mut grp = group(tr("TXT_NOTIFICATION_MESSAGE"), cx).child(
            row()
                .child(check(
                    "traffic-tip",
                    tr("TXT_TODAY_TRAFFIC_TIP_CHECK"),
                    g.traffic_tip_enable,
                    false,
                    false,
                    {
                        let m = m.clone();
                        move |v, w, cx| {
                            Self::update(&m, w, cx, |s, _, _| {
                                s.draft.general.traffic_tip_enable = v
                            })
                        }
                    },
                    cx,
                ))
                .child(stepper(
                    &self.steppers,
                    "traffic-tip-value",
                    g.traffic_tip_value,
                    1,
                    1_000_000,
                    10,
                    !g.traffic_tip_enable,
                    {
                        let m = m.clone();
                        move |v, w, cx| {
                            Self::update(&m, w, cx, |s, _, _| s.draft.general.traffic_tip_value = v)
                        }
                    },
                    window,
                    cx,
                ))
                .child(choice(
                    "traffic-tip-unit",
                    vec!["MB".into(), "GB".into()],
                    g.traffic_tip_unit.clamp(0, 1) as usize,
                    !g.traffic_tip_enable,
                    {
                        let m = m.clone();
                        move |i, w, cx| {
                            Self::update(&m, w, cx, |s, _, _| {
                                s.draft.general.traffic_tip_unit = i as i32
                            })
                        }
                    },
                ))
                .child(label(tr("TXT_TODAY_TRAFFIC_BACK"))),
        );
        let tips: [(
            &str,
            &str,
            &str,
            fn(&mut Settings) -> &mut crate::config::NotifyTip,
            crate::config::NotifyTip,
            bool,
        ); 5] = [
            (
                "mem-tip",
                "TXT_MEMORY_USAGE_TIP_CHECK",
                "TXT_MEMORY_USAGE_BACK",
                |s| &mut s.general.memory_usage_tip,
                g.memory_usage_tip,
                false,
            ),
            (
                "cpu-temp-tip",
                "TXT_CPU_TEMP_TIP_CHECK",
                "TXT_CPU_TEMP_BACK",
                |s| &mut s.general.cpu_temp_tip,
                g.cpu_temp_tip,
                true,
            ),
            (
                "gpu-temp-tip",
                "TXT_GPU_TEMP_TIP_CHECK",
                "TXT_GPU_TEMP_BACK",
                |s| &mut s.general.gpu_temp_tip,
                g.gpu_temp_tip,
                true,
            ),
            (
                "hdd-temp-tip",
                "TXT_HDD_TEMP_TIP_CHECK",
                "TXT_HDD_TEMP_BACK",
                |s| &mut s.general.hdd_temp_tip,
                g.hdd_temp_tip,
                true,
            ),
            (
                "mbd-temp-tip",
                "TXT_MBD_TEMP_TIP_CHECK",
                "TXT_MBD_TEMP_BACK",
                |s| &mut s.general.mainboard_temp_tip,
                g.mainboard_temp_tip,
                true,
            ),
        ];
        for (id, key, back, field, tip, needs_admin) in tips {
            let m1 = m.clone();
            let m2 = m.clone();
            let max = if needs_admin { 150 } else { 100 };
            grp = grp.child(
                row()
                    .child(check(
                        id,
                        tr(key),
                        tip.enable,
                        false,
                        needs_admin,
                        move |v, w, cx| {
                            Self::update(&m1, w, cx, |s, _, _| field(&mut s.draft).enable = v)
                        },
                        cx,
                    ))
                    .child(stepper(
                        &self.steppers,
                        format!("{id}-value"),
                        tip.tip_value,
                        1,
                        max,
                        1,
                        !tip.enable,
                        move |v, w, cx| {
                            Self::update(&m2, w, cx, |s, _, _| field(&mut s.draft).tip_value = v)
                        },
                        window,
                        cx,
                    ))
                    .child(label(tr(back))),
            );
        }
        grp.into_any_element()
    }

    fn hardware_group(&self, me: &WeakEntity<Self>, cx: &mut Context<Self>) -> AnyElement {
        let g = &self.draft.general;
        let m = me.clone();
        let (disks, cores, hw_state) = state::with(|s| {
            (
                s.disk_names.clone(),
                s.cpu_core_names.clone(),
                s.hardware_state.clone(),
            )
        });
        let items = [
            (HardwareItem::Cpu, "hw-cpu", "TXT_CPU"),
            (HardwareItem::Gpu, "hw-gpu", "TXT_GPU"),
            (HardwareItem::Hdd, "hw-hdd", "TXT_HARD_DISK"),
            (HardwareItem::MainBoard, "hw-mbd", "TXT_MAIN_BOARD"),
        ];
        let mut hw_row = row();
        for (item, id, key) in items {
            let m = m.clone();
            hw_row = hw_row.child(check(
                id,
                tr(key),
                g.is_hardware_enabled(item),
                false,
                true,
                move |v, w, cx| {
                    let m = m.clone();
                    if v {
                        let text = tr("IDS_HARDWARE_MONITOR_WARNING");
                        w.open_dialog(cx, move |dialog, _, _| {
                            let m = m.clone();
                            dialog
                                .confirm()
                                .title(tr("TXT_HARDWARE_MONITORING"))
                                .child(text.clone())
                                .on_ok(move |_, w, cx| {
                                    Self::update(&m, w, cx, |s, _, _| {
                                        s.draft.general.set_hardware_enabled(item, true)
                                    });
                                    true
                                })
                        });
                    } else {
                        Self::update(&m, w, cx, |s, _, _| {
                            s.draft.general.set_hardware_enabled(item, false)
                        });
                    }
                },
                cx,
            ));
        }
        let disk_index = disks
            .iter()
            .position(|d| *d == g.hard_disk_name)
            .unwrap_or(0);
        let core_index = cores
            .iter()
            .position(|c| *c == g.cpu_core_name)
            .unwrap_or(0);
        let status = match &hw_state {
            HardwareState::NotElevated if g.any_hardware_enabled() => {
                Some(tr("BAR_ADMIN_REQUIRED_TIP"))
            }
            HardwareState::Failed(e) => {
                Some(format!("{} {e}", tr("IDS_HARDWARE_MONITOR_INIT_FAILED")))
            }
            _ => None,
        };
        let disks_for_select = disks.clone();
        let cores_for_select = cores.clone();
        group(tr("TXT_HARDWARE_MONITORING"), cx)
            .child(hw_row)
            .child(
                row()
                    .child(label(tr("TXT_SELECT_HDD_STATIC")))
                    .child(choice(
                        "hdd-select",
                        disks,
                        disk_index,
                        disks_for_select.is_empty(),
                        {
                            let m = m.clone();
                            move |i, w, cx| {
                                let name = disks_for_select.get(i).cloned().unwrap_or_default();
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.general.hard_disk_name = name
                                })
                            }
                        },
                    )),
            )
            .child(
                row()
                    .child(label(tr("TXT_SELECT_CPU_STATIC")))
                    .child(choice(
                        "cpu-core-select",
                        cores,
                        core_index,
                        !g.is_hardware_enabled(HardwareItem::Cpu) || cores_for_select.is_empty(),
                        {
                            let m = m.clone();
                            move |i, w, cx| {
                                let name = cores_for_select.get(i).cloned().unwrap_or_default();
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.general.cpu_core_name = name
                                })
                            }
                        },
                    )),
            )
            .when_some(status, |grp, text| grp.child(muted(text, cx)))
            .into_any_element()
    }

    fn advanced_group(
        &self,
        me: &WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let g = &self.draft.general;
        let m = me.clone();
        group(tr("TXT_ADVANCED"), cx)
            .child(check(
                "show-all-interface",
                tr("TXT_SHOW_ALL_CONNECTION_CHECK"),
                g.show_all_interface,
                false,
                false,
                {
                    let m = m.clone();
                    move |v, w, cx| {
                        Self::update(&m, w, cx, |s, _, _| s.draft.general.show_all_interface = v)
                    }
                },
                cx,
            ))
            .child(muted(tr("IDS_SHOW_ALL_INFO_TIP"), cx))
            .child(row().child(button(
                "select-connections",
                tr_plain("TXT_SELECT_CONNECTIONS_BUTTON"),
                g.show_all_interface,
                {
                    let m = m.clone();
                    move |w, cx| {
                        Self::update(&m, w, cx, |s, w, cx| s.open_select_connections(w, cx))
                    }
                },
            )))
            .child(
                row()
                    .child(label(tr("TXT_CPU_ACQUISITION_METHOD")))
                    .child(radio(
                        "cpu-time",
                        tr("TXT_USE_CPU_TIME_RADIO"),
                        g.cpu_usage_acquire_method == 0,
                        false,
                        {
                            let m = m.clone();
                            move |w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.general.cpu_usage_acquire_method = 0
                                })
                            }
                        },
                    ))
                    .child(radio(
                        "cpu-pdh",
                        tr("TXT_USE_PDH_RADIO"),
                        g.cpu_usage_acquire_method != 0,
                        false,
                        {
                            let m = m.clone();
                            move |w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.general.cpu_usage_acquire_method = 1
                                })
                            }
                        },
                    )),
            )
            .child(
                row()
                    .child(label(tr("TXT_MONITORING_INTERVALS")))
                    .child(stepper(
                        &self.steppers,
                        "interval",
                        g.monitor_time_span,
                        MONITOR_TIME_SPAN_MIN,
                        MONITOR_TIME_SPAN_MAX,
                        100,
                        false,
                        {
                            let m = m.clone();
                            move |v, w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.general.monitor_time_span = v
                                })
                            }
                        },
                        window,
                        cx,
                    ))
                    .child(label(tr("TXT_MILLISECONDS")))
                    .child(button(
                        "interval-default",
                        tr_plain("TXT_RESTORE_DEFAULT_TIME_SPAN_BUTTON"),
                        false,
                        {
                            let m = m.clone();
                            move |w, cx| {
                                Self::update(&m, w, cx, |s, _, _| {
                                    s.draft.general.monitor_time_span = 1000
                                })
                            }
                        },
                    )),
            )
            .child(row().child(button(
                "plugin-manage",
                tr_plain("TXT_PLUGIN_MANAGE_BUTTON"),
                false,
                |_, cx| super::plugins::open_manager(cx),
            )))
            .into_any_element()
    }

    // ----------------------------------------------------------------- sub dialogs

    fn open_item_colors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.item_colors.clear();
        for (key, name) in item_names() {
            let c = self
                .draft
                .taskbar
                .text_colors
                .get(&key)
                .copied()
                .unwrap_or_default();
            let l = cx.new(|cx| {
                ColorPickerState::new(window, cx).default_value(colorref_to_hsla(c.label))
            });
            let v = cx.new(|cx| {
                ColorPickerState::new(window, cx).default_value(colorref_to_hsla(c.value))
            });
            self.item_colors.push((key, name, l, v));
        }
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let Some(view) = me.upgrade() else {
                return dialog;
            };
            let rows: Vec<AnyElement> = view
                .read(cx)
                .item_colors
                .iter()
                .map(|(_, name, l, v)| {
                    h_flex()
                        .gap_3()
                        .items_center()
                        .child(div().w(px(150.)).text_sm().child(name.clone()))
                        .child(label(tr("IDS_COLOR_LABEL")))
                        .child(ColorPicker::new(l).small())
                        .child(label(tr("IDS_COLOR_VALUE")))
                        .child(ColorPicker::new(v).small())
                        .into_any_element()
                })
                .collect();
            let me_ok = me.clone();
            dialog
                .title(tr("TXT_TITLE_TASKBAR_COLOR_DLG"))
                .w(px(560.))
                .child(
                    v_flex()
                        .id("item-colors-list")
                        .max_h(px(380.))
                        .overflow_y_scrollbar()
                        .gap_1()
                        .children(rows),
                )
                .confirm()
                .on_ok(move |_, w, cx| {
                    Self::update(&me_ok, w, cx, |s, _, cx| {
                        for (key, _, l, v) in &s.item_colors {
                            let label = l.read(cx).value().map(hsla_to_colorref).unwrap_or(0);
                            let value = v.read(cx).value().map(hsla_to_colorref).unwrap_or(0);
                            s.draft
                                .taskbar
                                .text_colors
                                .insert(key.clone(), crate::config::ItemColor { label, value });
                        }
                        s.style_modified = true;
                    });
                    true
                })
        });
    }

    fn open_display_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disp_inputs.clear();
        for (key, name) in item_names() {
            let text = self.draft.taskbar.label(&key);
            let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
            self.disp_inputs.push((key, name, input));
        }
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let Some(view) = me.upgrade() else {
                return dialog;
            };
            let rows: Vec<AnyElement> = view
                .read(cx)
                .disp_inputs
                .iter()
                .map(|(_, name, input)| {
                    h_flex()
                        .gap_3()
                        .items_center()
                        .child(div().w(px(150.)).text_sm().child(name.clone()))
                        .child(div().flex_1().child(Input::new(input).small()))
                        .into_any_element()
                })
                .collect();
            let me_ok = me.clone();
            let me_default = me.clone();
            dialog
                .title(tr("TXT_TITLE_DISPLAY_TEXT_SETTING"))
                .w(px(480.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            v_flex()
                                .id("disp-text-list")
                                .max_h(px(360.))
                                .overflow_y_scrollbar()
                                .gap_1()
                                .children(rows),
                        )
                        .child(row().child(button(
                            "disp-default",
                            tr_plain("TXT_RESTORE_DEFAULT"),
                            false,
                            move |w, cx| {
                                Self::update(&me_default, w, cx, |s, w, cx| {
                                    let plugin_defaults: Vec<(String, String)> =
                                        state::with(|st| st.plugins.item_labels());
                                    for (key, _, input) in &s.disp_inputs {
                                        let text = match key {
                                            ItemKey::Builtin(d) => d.default_label(),
                                            ItemKey::Plugin(id) => plugin_defaults
                                                .iter()
                                                .find(|p| &p.0 == id)
                                                .map(|p| p.1.clone())
                                                .unwrap_or_default(),
                                        };
                                        input.update(cx, |i, cx| i.set_value(text, w, cx));
                                    }
                                })
                            },
                        ))),
                )
                .confirm()
                .on_ok(move |_, w, cx| {
                    Self::update(&me_ok, w, cx, |s, _, cx| {
                        for (key, _, input) in &s.disp_inputs {
                            let text = input.read(cx).value().to_string();
                            s.draft.taskbar.disp_str.insert(key.clone(), text);
                        }
                    });
                    true
                })
        });
    }

    fn open_item_order(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.order_model = Some(ItemOrderModel::from_settings(
            &self.draft.taskbar,
            hardware_filter(&self.draft.general),
        ));
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let Some(view) = me.upgrade() else {
                return dialog;
            };
            let Some(model) = view.read(cx).order_model.clone() else {
                return dialog;
            };
            let me_up = me.clone();
            let update = move |w: &mut Window, cx: &mut App, f: &dyn Fn(&mut ItemOrderModel)| {
                Self::update(&me_up, w, cx, |s, _, _| {
                    if let Some(m) = s.order_model.as_mut() {
                        f(m);
                    }
                })
            };
            let me_ok = me.clone();
            dialog
                .title(tr("TXT_TITLE_SELECT_ORDER_DIALOG"))
                .w(px(440.))
                .child(model.render("opt-order", update, cx))
                .confirm()
                .on_ok(move |_, w, cx| {
                    Self::update(&me_ok, w, cx, |s, _, _| {
                        if let Some(m) = s.order_model.take() {
                            let count =
                                state::with(|st| st.plugins.item_ids().len()) + ALL_BUILTIN.len();
                            m.apply_to(&mut s.draft.taskbar, count);
                        }
                    });
                    true
                })
        });
    }

    fn open_auto_adapt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let Some(view) = me.upgrade() else {
                return dialog;
            };
            let t = view.read(cx).draft.taskbar.clone();
            let presets: Vec<String> = (0..crate::config::TASKBAR_DEFAULT_STYLE_NUM)
                .map(|i| format!("{} {}", tr("IDS_PRESET"), i + 1))
                .collect();
            let m1 = me.clone();
            let m2 = me.clone();
            let m3 = me.clone();
            dialog
                .title(tr("TXT_TITLE_AUTO_ADATP_SETTINGS"))
                .w(px(460.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            row()
                                .child(label(tr("TXT_COLOR_PRESET_IN_DARK_MODE")))
                                .child(choice(
                                    "dark-preset",
                                    presets.clone(),
                                    t.dark_default_style.clamp(0, 3) as usize,
                                    false,
                                    move |i, w, cx| {
                                        Self::update(&m1, w, cx, |s, _, _| {
                                            s.draft.taskbar.dark_default_style = i as i32
                                        })
                                    },
                                )),
                        )
                        .child(
                            row()
                                .child(label(tr("TXT_COLOR_PRESET_IN_LIGHT_MODE")))
                                .child(choice(
                                    "light-preset",
                                    presets,
                                    t.light_default_style.clamp(0, 3) as usize,
                                    false,
                                    move |i, w, cx| {
                                        Self::update(&m2, w, cx, |s, _, _| {
                                            s.draft.taskbar.light_default_style = i as i32
                                        })
                                    },
                                )),
                        )
                        .child(check(
                            "auto-save-preset",
                            tr("TXT_AUTO_SAVE_TO_PRESET_CHECK"),
                            t.auto_save_taskbar_color_settings_to_preset,
                            false,
                            false,
                            move |v, w, cx| {
                                Self::update(&m3, w, cx, |s, _, _| {
                                    s.draft.taskbar.auto_save_taskbar_color_settings_to_preset = v
                                })
                            },
                            cx,
                        ))
                        .child(muted(tr("IDS_AUTO_SAVE_TO_PRESET_TIP"), cx)),
                )
                .alert()
        });
    }

    fn open_win11(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let Some(view) = me.upgrade() else {
                return dialog;
            };
            let host = view.read(cx).steppers.clone();
            let t = view.read(cx).draft.taskbar.clone();
            let (m1, m2, m3, m4, m5, m6) = (
                me.clone(),
                me.clone(),
                me.clone(),
                me.clone(),
                me.clone(),
                me.clone(),
            );
            dialog
                .title(tr("TXT_TITLE_WIN11_TASKBAR_SETTING"))
                .w(px(520.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(check(
                            "snap",
                            tr("TXT_TASKBAR_WINDOWS_CLOSE_TO_ICON"),
                            t.tbar_wnd_snap,
                            false,
                            false,
                            move |v, w, cx| {
                                Self::update(&m1, w, cx, |s, _, _| {
                                    s.draft.taskbar.tbar_wnd_snap = v
                                })
                            },
                            cx,
                        ))
                        .child(
                            row()
                                .child(label(tr("TXT_WINDOW_VERTICAL_OFFSET")))
                                .child(stepper(
                                    &host,
                                    "offset-top",
                                    t.window_offset_top,
                                    -20,
                                    20,
                                    1,
                                    false,
                                    move |v, w, cx| {
                                        Self::update(&m2, w, cx, |s, _, _| {
                                            s.draft.taskbar.window_offset_top = v
                                        })
                                    },
                                    window,
                                    cx,
                                ))
                                .child(label(tr("TXT_PIXELS"))),
                        )
                        .child(
                            row()
                                .child(label(tr("TXT_WINDOW_HORIZONTAL_OFFSET")))
                                .child(stepper(
                                    &host,
                                    "offset-left",
                                    t.window_offset_left,
                                    -800,
                                    800,
                                    1,
                                    false,
                                    move |v, w, cx| {
                                        Self::update(&m3, w, cx, |s, _, _| {
                                            s.draft.taskbar.window_offset_left = v
                                        })
                                    },
                                    window,
                                    cx,
                                ))
                                .child(label(tr("TXT_PIXELS"))),
                        )
                        .child(check(
                            "avoid-widgets",
                            tr("TXT_AVOID_OVERLAP_RIGHT_WIDGETS_CHECK"),
                            t.avoid_overlap_with_widgets,
                            false,
                            false,
                            move |v, w, cx| {
                                Self::update(&m4, w, cx, |s, _, _| {
                                    s.draft.taskbar.avoid_overlap_with_widgets = v
                                })
                            },
                            cx,
                        ))
                        .child(
                            row()
                                .child(label(tr("TXT_WIDGETS_WIDTH")))
                                .child(stepper(
                                    &host,
                                    "widgets-width",
                                    t.taskbar_left_space_win11,
                                    0,
                                    1000,
                                    1,
                                    false,
                                    move |v, w, cx| {
                                        Self::update(&m5, w, cx, |s, _, _| {
                                            s.draft.taskbar.taskbar_left_space_win11 = v
                                        })
                                    },
                                    window,
                                    cx,
                                ))
                                .child(label(tr("TXT_PIXELS"))),
                        )
                        .child(row().child(button(
                            "win11-default",
                            tr_plain("TXT_RESTORE_DEFAULT"),
                            false,
                            move |w, cx| {
                                Self::update(&m6, w, cx, |s, _, _| {
                                    let t = &mut s.draft.taskbar;
                                    t.tbar_wnd_snap = false;
                                    t.window_offset_top = 0;
                                    t.window_offset_left = 0;
                                    t.avoid_overlap_with_widgets = false;
                                    t.taskbar_left_space_win11 = 160;
                                })
                            },
                        ))),
                )
                .alert()
        });
    }

    fn open_select_connections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.adapters = crate::monitor::net::all_adapter_descriptions();
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let Some(view) = me.upgrade() else {
                return dialog;
            };
            let v = view.read(cx);
            let hidden = v.draft.general.connections_hide.clone();
            let rows: Vec<AnyElement> = v
                .adapters
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let m = me.clone();
                    let n = name.clone();
                    check(
                        format!("conn-{i}"),
                        name.clone(),
                        !hidden.contains(name),
                        false,
                        false,
                        move |on, w, cx| {
                            let n = n.clone();
                            Self::update(&m, w, cx, |s, _, _| {
                                s.draft.general.connections_hide.retain(|h| *h != n);
                                if !on {
                                    s.draft.general.connections_hide.push(n);
                                }
                            })
                        },
                        cx,
                    )
                })
                .collect();
            dialog
                .title(tr("TXT_TITLE_SELECTION_CONNECTION"))
                .w(px(460.))
                .child(
                    v_flex()
                        .id("conn-list")
                        .max_h(px(360.))
                        .overflow_y_scrollbar()
                        .gap_1()
                        .children(rows),
                )
                .alert()
        });
    }
}

impl Render for OptionsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let me = cx.entity().downgrade();
        let tab_me = me.clone();
        let side_gap = px(16.);
        let side_w = px(230.);
        let page: Vec<AnyElement> = if self.tab == 0 {
            vec![
                self.color_font_group(&me, window, cx),
                h_flex()
                    .w_full()
                    .gap(side_gap)
                    .items_start()
                    .child(self.display_group(&me, window, cx))
                    .child(self.unit_group(&me, cx, side_w))
                    .into_any_element(),
                self.taskbar_wnd_group(&me, cx),
                self.graph_group(&me, window, cx),
                self.render_group(&me, cx),
            ]
        } else {
            vec![
                h_flex()
                    .w_full()
                    .gap(side_gap)
                    .items_start()
                    .child(self.app_group(&me, cx))
                    .child(
                        v_flex()
                            .w(side_w)
                            .min_w(side_w)
                            .max_w(side_w)
                            .flex_shrink_0()
                            .gap_4()
                            .child(self.data_group(&me, cx, side_w))
                            .child(self.notify_icon_group(&me, cx, side_w)),
                    )
                    .into_any_element(),
                self.auto_run_group(&me, cx),
                self.memory_clean_group(&me, window, cx),
                self.notify_group(&me, window, cx),
                self.hardware_group(&me, cx),
                self.advanced_group(&me, window, cx),
            ]
        };
        let ok_me = me.clone();
        let apply_me = me.clone();
        v_flex()
            .size_full()
            .child(
                div().px_4().pt_2().child(
                    TabBar::new("options-tabs")
                        .underline()
                        .selected_index(self.tab)
                        .on_click(move |ix, w, cx| {
                            Self::update(&tab_me, w, cx, |s, _, _| s.tab = *ix)
                        })
                        .child(Tab::new().label(tr("IDS_TASKBAR_WINDOW_SETTINGS")))
                        .child(Tab::new().label(tr("IDS_GENERAL_SETTINGS"))),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        v_flex()
                            .id(("options-page", self.tab))
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.page_scroll)
                            .p_4()
                            .gap_4()
                            .children(page),
                    )
                    .child(thin_vscroll(&self.page_scroll)),
            )
            .child(footer(
                vec![
                    primary_button("opt-ok", tr("TXT_OK"), move |w, cx| {
                        Self::update(&ok_me, w, cx, |s, w, cx| s.apply(w, cx));
                        w.remove_window();
                    }),
                    button("opt-cancel", tr("TXT_CANCEL"), false, |w, _| {
                        w.remove_window()
                    }),
                    button("opt-apply", tr("TXT_APPLY"), false, move |w, cx| {
                        Self::update(&apply_me, w, cx, |s, w, cx| s.apply(w, cx));
                    }),
                ],
                cx,
            ))
    }
}

pub fn open(tab: usize, cx: &mut App) {
    super::open_window(
        Slot::Options,
        tr("TXT_TITLE_OPTION"),
        size(px(760.), px(600.)),
        true,
        cx,
        move |window, cx| OptionsView::new(tab, window, cx),
    );
}
