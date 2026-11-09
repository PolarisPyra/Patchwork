//! The controls every settings pane draws: a row, a switch, a row of
//! choices, a text field, a dropdown, and the badges around them.

// Rust guideline compliant 2026-10-09

use std::rc::Rc;

use gpui::{
    AnyElement, Div, Entity, Focusable, FontWeight, Hsla, Role, SharedString, Stateful, Toggled,
    Window, div, hsla, prelude::*, px,
};

use crate::chat::text_area::{BackspaceEmpty, Changed, Submitted, TextArea};
use crate::ui::menu::{Dismissed, Menu, MenuItem};
use crate::settings::SettingsDialog;
use crate::ui::theme::{FONT_MONO, FS_MD, FS_SM, RADIUS_SM, Theme, theme};
use crate::ui::{button, icon};

/// What a text field does with its text when it is left or Enter is pressed.
pub type Save = Rc<dyn Fn(&mut SettingsDialog, String, &mut Context<SettingsDialog>)>;

/// What Backspace does in a field that is already empty.
type Back = Rc<dyn Fn(&mut SettingsDialog, &mut Context<SettingsDialog>)>;

/// A text field of a pane: its editor, and the value it last showed from the settings.
pub struct Field {
    area: Entity<TextArea>,
    shown: String,
    save: Save,
    /// What Enter does in a live field; without it, Enter leaves the field.
    submit: Option<Save>,
    /// What Backspace does in a live field that is empty.
    back: Option<Back>,
}

/// How a field draws and when it saves: on leaving it, or `live` at each change, as a form's
/// draft does.
#[derive(Clone, Copy, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate way to draw or save the field"
)]
pub struct FieldKind {
    pub masked: bool,
    pub live: bool,
    /// More than one line, as a system prompt.
    pub lines: bool,
    pub mono: bool,
}

/// A letter on a colour picked from the name, so each row is easy to spot.
pub fn letter_badge(name: &str, size: f32) -> Div {
    let mut hash: i32 = 0;
    for c in name.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(c));
    }
    let hue = (hash.unsigned_abs() % 360) as f32 / 360.;
    let letter = name
        .chars()
        .find(char::is_ascii_alphanumeric)
        .map_or_else(|| "?".to_string(), |c| c.to_uppercase().to_string());
    div()
        .flex_none()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .bg(hsla(hue, 0.7, 0.55, 0.18))
        .text_color(hsla(hue, 0.75, 0.65, 1.))
        .text_size(px(size * 0.45))
        .font_weight(FontWeight::SEMIBOLD)
        .child(letter)
}

/// One setting: its name and what it does, with the control on the right.
pub fn token_row(title: impl Into<SharedString>, desc: Option<&str>, t: &Theme) -> Stateful<Div> {
    let title = title.into();
    div()
        .id(SharedString::from(format!("row-{title}")))
        .role(Role::Group)
        .aria_label(title.clone())
        .flex()
        .items_center()
        .gap(px(12.))
        .border_b_1()
        .border_color(t.border)
        .py(px(10.))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .children(desc.map(|desc| {
                    div()
                        .text_size(px(FS_SM))
                        .text_color(t.text_muted)
                        .child(desc.to_string())
                })),
        )
}

/// A heading over a run of rows, for a pane that holds more than one kind of setting.
pub fn section_heading(text: impl Into<SharedString>, first: bool, t: &Theme) -> Div {
    div()
        .when(!first, |d| d.mt(px(16.)))
        .mb(px(2.))
        .text_size(px(FS_SM))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.text_subtle)
        .child(text.into())
}

/// `.toggle`: a switch, its knob at the right when on.
pub fn switch(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    on: bool,
    t: &Theme,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .role(Role::Switch)
        .aria_label(label.into())
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .tab_index(0)
        .focus_visible(crate::ui::focus_ring)
        .cursor_pointer()
        .relative()
        .flex_none()
        .w(px(34.))
        .h(px(20.))
        .rounded(px(10.))
        .border_1()
        .border_color(if on { t.accent } else { t.border_strong })
        .bg(if on { t.accent } else { t.bg_selected_muted })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 16. } else { 2. }))
                .size(px(14.))
                .rounded_full()
                .bg(t.text_on_accent)
                .shadow(vec![gpui::BoxShadow {
                    color: hsla(0., 0., 0., 0.3),
                    offset: gpui::point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                    inset: false,
                }]),
        )
}

/// Two to five choices side by side, as the theme picker. Each choice is `(label, checked)`;
/// the caller adds the click to each with `pick`.
pub fn segmented<T: Copy + PartialEq + 'static>(
    id: &str,
    label: impl Into<SharedString>,
    value: T,
    options: &[(T, impl Into<SharedString> + Clone)],
    t: &Theme,
    pick: impl Fn(&mut SettingsDialog, T, &mut Context<SettingsDialog>) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let pick = Rc::new(pick);
    let (hover, fg, muted) = (t.bg_hover, t.text, t.text_muted);
    div()
        .id(SharedString::from(format!("{id}-group")))
        .role(Role::RadioGroup)
        .aria_label(label.into())
        // Every choice as wide as the widest (`auto-cols-fr`).
        .grid()
        .grid_cols_min_content(options.len() as u16)
        .flex_none()
        .overflow_hidden()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(t.border_strong)
        .children(options.iter().enumerate().map(|(i, (option, text))| {
            let (option, text): (T, SharedString) = (*option, text.clone().into());
            let checked = option == value;
            let pick = Rc::clone(&pick);
            div()
                .id(SharedString::from(format!("{id}-{i}")))
                .role(Role::RadioButton)
                .aria_label(text.clone())
                .aria_toggled(if checked {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .tab_index(0)
                .focus_visible(crate::ui::focus_ring)
                .cursor_pointer()
                .flex()
                .h(px(24.))
                .items_center()
                .justify_center()
                .px(px(16.))
                .whitespace_nowrap()
                .text_size(px(FS_SM))
                .map(|d| {
                    if checked {
                        d.bg(t.bg_selected).text_color(fg)
                    } else {
                        d.text_color(muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                    }
                })
                .child(text)
                .on_click(cx.listener(move |d, _, _, cx| pick(d, option, cx)))
        }))
}

/// A chip that shows what is picked and opens a menu of the choices (`LogLevelPicker` and the
/// other dropdowns): `.chip`, a borderless button with its value and a caret.
pub fn dropdown_chip(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    current: impl Into<SharedString>,
    open: bool,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(id.into(), label)
        .aria_expanded(open)
        .flex_none()
        .h(px(28.))
        .gap(px(4.))
        .rounded(px(RADIUS_SM))
        .px(px(6.))
        .text_size(px(FS_MD))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(div().max_w(px(180.)).truncate().child(current.into()))
        .child(icon("caret-down", 14.))
}

/// A `<select>`: a bordered box with what is picked and a caret, which opens a menu of the choices.
pub fn select_box(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    current: impl Into<SharedString>,
    open: bool,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(id.into(), label)
        .aria_expanded(open)
        .flex_none()
        .h(px(28.))
        .justify_between()
        .gap(px(6.))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_editor)
        .pl(px(8.))
        .pr(px(6.))
        .text_size(px(FS_SM))
        .text_color(t.text)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(div().min_w_0().truncate().child(current.into()))
        .child(icon("caret-down", 12.))
}

/// The input every pane shares: one height, border and focus colour.
pub fn input_box(focused: bool, t: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(28.))
        .min_w_0()
        .overflow_hidden()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(if focused { t.accent } else { t.border_strong })
        .bg(t.bg_editor)
        .px(px(8.))
        .text_size(px(FS_SM))
        .text_color(t.text)
}

impl SettingsDialog {
    /// A text field that saves when it loses focus or on Enter (`TextField`). `key` names it
    /// across renders; the settings' value replaces what it shows whenever it changes while the
    /// field is not being edited.
    #[expect(
        clippy::too_many_arguments,
        reason = "each argument is a separate piece of what it draws"
    )]
    pub fn text_field(
        &mut self,
        key: impl Into<SharedString>,
        label: impl Into<SharedString>,
        value: &str,
        placeholder: &str,
        width: Option<f32>,
        save: impl Fn(&mut Self, String, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.field(
            key,
            label,
            value,
            placeholder,
            FieldKind::default(),
            save,
            window,
            cx,
        )
        .map(|d| match width {
            Some(w) => d.w(px(w)).flex_none(),
            None => d.w_full(),
        })
        .into_any_element()
    }

    /// A field as `text_field`, drawn and saved as `kind` says.
    #[expect(
        clippy::too_many_arguments,
        reason = "each argument is a separate piece of what it draws"
    )]
    pub fn field(
        &mut self,
        key: impl Into<SharedString>,
        label: impl Into<SharedString>,
        value: &str,
        placeholder: &str,
        kind: FieldKind,
        save: impl Fn(&mut Self, String, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = key.into();
        let t = *theme(cx);
        let save: Save = Rc::new(save);
        if !self.fields.contains_key(&key) {
            let label = label.into();
            let placeholder = SharedString::from(placeholder.to_string());
            let area = cx.new(|cx| {
                let mut area = TextArea::new(placeholder, label, cx);
                area.single_line = !kind.lines;
                area.masked = kind.masked;
                area.text_color = t.text;
                area.placeholder_color = crate::ui::theme::input_placeholder();
                area.selection_color = t.accent.opacity(0.3);
                area.set_text(value, cx);
                area
            });
            let focus = area.read(cx).focus_handle(cx);
            let k = key.clone();
            if kind.live {
                let changed = k.clone();
                cx.subscribe(&area, move |d, _, _: &Changed, cx| {
                    d.commit_field(&changed, cx);
                })
                .detach();
                cx.subscribe_in(
                    &area,
                    window,
                    move |d, area, _: &Submitted, window, cx| match d
                        .fields
                        .get(&k)
                        .and_then(|f| f.submit.clone())
                    {
                        Some(submit) => submit(d, area.read(cx).content.clone(), cx),
                        None => window.blur(cx),
                    },
                )
                .detach();
                let back = key.clone();
                cx.subscribe(&area, move |d, _, _: &BackspaceEmpty, cx| {
                    if let Some(back) = d.fields.get(&back).and_then(|f| f.back.clone()) {
                        back(d, cx);
                    }
                })
                .detach();
            } else {
                cx.subscribe_in(&area, window, move |_, _, _: &Submitted, window, cx| {
                    window.blur(cx);
                })
                .detach();
                cx.on_focus_out(&focus, window, move |d, _, _, cx| d.commit_field(&k, cx))
                    .detach();
            }
            self.fields.insert(
                key.clone(),
                Field {
                    area,
                    shown: value.to_string(),
                    save: Rc::clone(&save),
                    submit: None,
                    back: None,
                },
            );
        }
        let Some(field) = self.fields.get_mut(&key) else {
            return input_box(false, &t);
        };
        field.save = save;
        let focused = field.area.read(cx).focus_handle(cx).is_focused(window);
        if !focused && field.shown != value {
            field.shown = value.to_string();
            field.area.update(cx, |a, cx| a.set_text(value, cx));
        }
        if field.area.read(cx).placeholder.as_ref() != placeholder {
            let placeholder = SharedString::from(placeholder.to_string());
            field.area.update(cx, |a, cx| {
                a.placeholder = placeholder;
                cx.notify();
            });
        }
        input_box(focused, &t)
            .when(kind.lines, |d| {
                d.h_auto().min_h(px(70.)).items_start().py(px(6.))
            })
            .when(kind.mono, |d| d.font_family(FONT_MONO))
            .child(div().flex_1().min_w_0().child(field.area.clone()))
    }

    /// The editor of the field named `key`, once it has been drawn.
    pub fn field_area(&self, key: &str) -> Option<Entity<TextArea>> {
        self.fields.get(key).map(|f| f.area.clone())
    }

    /// Gives the live field named `key` what Enter does.
    pub fn on_submit(
        &mut self,
        key: &str,
        submit: impl Fn(&mut Self, String, &mut Context<Self>) + 'static,
    ) {
        if let Some(field) = self.fields.get_mut(key) {
            field.submit = Some(Rc::new(submit));
        }
    }

    /// Gives the live field named `key` what Backspace does when it is empty.
    pub fn on_back(&mut self, key: &str, back: impl Fn(&mut Self, &mut Context<Self>) + 'static) {
        if let Some(field) = self.fields.get_mut(key) {
            field.back = Some(Rc::new(back));
        }
    }

    /// Forgets the fields whose names start with `prefix`, so a form opened again starts from its
    /// values.
    pub fn drop_fields(&mut self, prefix: &str) {
        self.fields.retain(|key, _| !key.starts_with(prefix));
    }

    /// Saves a field's text when it differs from what the settings hold.
    fn commit_field(&mut self, key: &SharedString, cx: &mut Context<Self>) {
        let Some(field) = self.fields.get(key) else {
            return;
        };
        let text = field.area.read(cx).content.clone();
        if text == field.shown {
            return;
        }
        let save = Rc::clone(&field.save);
        if let Some(field) = self.fields.get_mut(key) {
            field.shown.clone_from(&text);
        }
        save(self, text, cx);
        cx.notify();
    }

    /// A number field that saves a valid number when it loses focus or on Enter.
    #[expect(
        clippy::too_many_arguments,
        reason = "each argument is a separate piece of what it draws"
    )]
    pub fn number_field(
        &mut self,
        key: impl Into<SharedString>,
        label: impl Into<SharedString>,
        value: f64,
        step: f64,
        min: Option<f64>,
        max: Option<f64>,
        save: impl Fn(&mut Self, f64, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown = if value.fract() == 0. {
            format!("{}", value as i64)
        } else {
            format!("{value}")
        };
        self.text_field(
            key,
            label,
            &shown,
            "",
            Some(80.),
            move |d, text, cx| {
                let Ok(n) = text.trim().parse::<f64>() else {
                    return;
                };
                if !n.is_finite() {
                    return;
                }
                let n = n.clamp(min.unwrap_or(f64::MIN), max.unwrap_or(f64::MAX));
                save(d, if step >= 1. { n.round() } else { n }, cx);
            },
            window,
            cx,
        )
    }

    /// Opens the menu of a dropdown named `key` under the pointer, or closes it when it is open.
    pub fn toggle_menu(
        &mut self,
        key: impl Into<SharedString>,
        label: impl Into<SharedString>,
        items: Vec<MenuItem>,
        at: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = key.into();
        if self.menu.as_ref().is_some_and(|(k, _, _)| *k == key) {
            self.menu = None;
            cx.notify();
            return;
        }
        let menu = Menu::open(label, items, window, cx);
        cx.subscribe_in(&menu, window, |d, _, _: &Dismissed, window, cx| {
            d.menu = None;
            window.focus(&d.focus, cx);
            cx.notify();
        })
        .detach();
        self.menu = Some((key, menu, at));
        cx.notify();
    }

    /// Whether the dropdown named `key` has its menu open.
    pub fn menu_open(&self, key: &str) -> bool {
        self.menu.as_ref().is_some_and(|(k, _, _)| k == key)
    }
}

/// A menu row that runs `pick` on the dialog, marked when it is the current choice.
pub fn choice(
    dialog: gpui::WeakEntity<SettingsDialog>,
    label: impl Into<SharedString>,
    checked: bool,
    pick: impl Fn(&mut SettingsDialog, &mut Context<SettingsDialog>) + 'static,
) -> MenuItem {
    let item = MenuItem::new(label, move |_, cx| {
        let _ = dialog.update(cx, |d, cx| pick(d, cx));
    });
    if checked { item.icon("check") } else { item }
}

/// The minutes between update checks that the stepper offers.
const CHECK_MINUTES: [u32; 6] = [10, 20, 30, 40, 50, 60];

/// Steps through `CHECK_MINUTES` (`MinutesStepper`).
pub fn minutes_stepper(
    label: &str,
    value: u32,
    t: &Theme,
    change: impl Fn(&mut SettingsDialog, u32, &mut Context<SettingsDialog>) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let at = CHECK_MINUTES.iter().position(|&m| m == value).unwrap_or(0);
    let change = Rc::new(change);
    let (hover, fg, muted) = (t.bg_hover, t.text, t.text_muted);
    let step = |id: &'static str, name: &'static str, glyph: &str, to: Option<usize>| {
        let change = Rc::clone(&change);
        button(id, name)
            .size(px(24.))
            .justify_center()
            .text_color(muted)
            .child(icon(glyph, 12.))
            .map(|b| match to {
                Some(to) => b
                    .hover(move |s| s.bg(hover).text_color(fg))
                    .on_click(cx.listener(move |d, _, _, cx| change(d, CHECK_MINUTES[to], cx))),
                None => b.opacity(0.4).cursor_default(),
            })
    };
    let shown = if CHECK_MINUTES[at] == 60 {
        "1 hr".to_string()
    } else {
        format!("{} min", CHECK_MINUTES[at])
    };
    div()
        .id("minutes-stepper")
        .role(Role::Group)
        .aria_label(label.to_string())
        .flex()
        .flex_none()
        .items_center()
        .overflow_hidden()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(t.border_strong)
        .child(step("less-often", "Less often", "minus", at.checked_sub(1)))
        .child(
            div()
                .w(px(56.))
                .border_x_1()
                .border_color(t.border_strong)
                .text_center()
                .text_size(px(FS_SM))
                .child(shown),
        )
        .child(step(
            "more-often",
            "More often",
            "plus",
            (at + 1 < CHECK_MINUTES.len()).then_some(at + 1),
        ))
}

/// A line in red under what failed (`role="alert"`).
pub fn alert(
    id: impl Into<SharedString>,
    text: impl Into<SharedString>,
    color: Hsla,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .role(Role::Alert)
        .pt(px(8.))
        .text_size(px(FS_SM))
        .text_color(color)
        .child(text.into())
}

/// A filter chip, filled when it is the one showing (`chip`). The caller adds what it shows.
pub fn chip(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    on: bool,
    t: &Theme,
) -> Stateful<Div> {
    let hover = t.bg_hover;
    button(id.into(), label)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .flex_none()
        .h(px(28.))
        .gap(px(6.))
        .px(px(10.))
        .rounded_full()
        .border_1()
        .whitespace_nowrap()
        .text_size(px(FS_SM))
        .text_color(t.text)
        .map(|b| {
            if on {
                b.border_color(t.accent).bg(t.bg_accent_soft)
            } else {
                b.border_color(t.border).hover(move |s| s.bg(hover))
            }
        })
}

/// The rows of a list in one bordered box, a line between each.
pub fn list_box(rows: Vec<AnyElement>, t: &Theme) -> Div {
    let border = t.border;
    div()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(crate::ui::theme::RADIUS))
        .border_1()
        .border_color(border)
        .children(rows.into_iter().enumerate().map(move |(i, row)| {
            div()
                .when(i > 0, |d| d.border_t_1().border_color(border))
                .child(row)
        }))
}
