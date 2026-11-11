//! The text and number fields of a settings pane: what each holds, saving it, and the dropdown a pane has open.

// Rust guideline compliant 2026-10-09

use std::rc::Rc;
use gpui::{AnyElement, Div, Entity, Focusable, SharedString, Window, div, prelude::*, px};
use crate::ui::text_area::{BackspaceEmpty, Changed, Submitted, TextArea};
use crate::ui::menu::{Menu, MenuItem};
use crate::settings::SettingsDialog;
use crate::ui::theme::{FONT_MONO, theme};
use crate::ui::input_box;

/// What a text field does with its text when it is left or Enter is pressed.
pub type Save = Rc<dyn Fn(&mut SettingsDialog, String, &mut Context<SettingsDialog>)>;

/// What Backspace does in a field that is already empty.
pub(super) type Back = Rc<dyn Fn(&mut SettingsDialog, &mut Context<SettingsDialog>)>;

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
        let menu = Menu::open_for(label, items, window, cx, |d: &mut Self, window, cx| {
            d.menu = None;
            window.focus(&d.focus, cx);
            cx.notify();
        });
        self.menu = Some((key, menu, at));
        cx.notify();
    }

    /// Whether the dropdown named `key` has its menu open.
    pub fn menu_open(&self, key: &str) -> bool {
        self.menu.as_ref().is_some_and(|(k, _, _)| k == key)
    }
}
