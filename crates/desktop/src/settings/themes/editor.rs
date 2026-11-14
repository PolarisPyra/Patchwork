//! The editor of Settings → Themes: the colors of one color scheme's file, changed with a
//! color written out or three sliders, and written to the file as they change.
//!
//! The file is watched, so the app is drawn in the new colors while the scheme is the one
//! showing. What is hard to read in it is listed under the sliders.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{
    AnyElement, Hsla, Rgba, Role, SharedString, Toggled, Window, div, hsla, prelude::*, px, rgb,
};
use patchwork_server::themes::{Color, ThemeFile};

use super::slider::{Range, slider};
use crate::settings::SettingsDialog;
use crate::settings::controls::{alert, token_row};
use crate::ui::theme::{FS_SM, Theme as Colors, radius, radius_sm, theme};
use crate::ui::{BtnKind, btn, focus_ring};

/// A color of a color scheme that the editor changes.
struct Token {
    name: &'static str,
    /// The color the file names for it. None is one the file leaves to be mixed.
    get: fn(&ThemeFile) -> Option<Color>,
    set: fn(&mut ThemeFile, Option<Color>),
    /// The color it is drawn in, named or mixed.
    shown: fn(&Colors) -> Hsla,
    /// It is one of the four every other color is mixed from, which a file always names.
    anchor: bool,
}

macro_rules! anchor {
    ($name:literal, $field:ident, $shown:ident) => {
        Token {
            name: $name,
            get: |file| Some(file.colors.$field),
            set: |file, color| {
                if let Some(color) = color {
                    file.colors.$field = color;
                }
            },
            shown: |palette| palette.$shown,
            anchor: true,
        }
    };
}

macro_rules! named {
    ($name:literal, $field:ident) => {
        Token {
            name: $name,
            get: |file| file.overrides.$field,
            set: |file, color| file.overrides.$field = color,
            shown: |palette| palette.$field,
            anchor: false,
        }
    };
}

const TOKENS: [Token; 21] = [
    anchor!("Surface", surface, bg_app),
    anchor!("Text", text, text),
    anchor!("Faint text", muted, text_muted),
    anchor!("Accent", accent, accent),
    named!("Panels", bg_panel),
    named!("Reading area", bg_editor),
    named!("Inputs", bg_input),
    named!("Hover", bg_hover),
    named!("Selected", bg_selected),
    named!("Selected, unfocused", bg_selected_muted),
    named!("Popups", bg_elevated),
    named!("Border", border),
    named!("Strong border", border_strong),
    named!("Faintest text", text_subtle),
    named!("Text on accent", text_on_accent),
    named!("Accent hover", accent_hover),
    named!("Links", accent_text),
    named!("Success", success),
    named!("Danger", danger),
    named!("Warning", warning),
    named!("Your messages", bubble),
];

/// The three numbers a color is slid by, and what each goes up to.
const SLIDES: [(&str, f32); 3] = [("Hue", 360.), ("Saturation", 100.), ("Lightness", 100.)];

/// The color scheme being edited.
pub(super) struct Editing {
    id: String,
    file: ThemeFile,
    /// Which of `TOKENS` the sliders change.
    token: usize,
    /// The hue, saturation and lightness the sliders show. Kept apart from the color, which
    /// forgets its hue once it is gray.
    slides: [f32; 3],
    /// The file is being written, and it changed again since.
    saving: bool,
    dirty: bool,
}

impl Editing {
    /// Shows the color of the token being edited on the sliders.
    fn read_slides(&mut self) {
        let shown = (TOKENS[self.token].shown)(&Colors::from_file(&self.file));
        self.slides = [shown.h * 360., shown.s * 100., shown.l * 100.];
    }
}

/// The color `shown` as a file writes it.
fn code(shown: Hsla) -> Color {
    let Rgba { r, g, b, .. } = shown.into();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a channel is between zero and one"
    )]
    let [r, g, b] = [r, g, b].map(|c| (c.clamp(0., 1.) * 255.).round() as u32);
    Color(r << 16 | g << 8 | b)
}

/// The color written as `#rrggbb`, with or without its `#`.
fn written(text: &str) -> Option<Color> {
    let digits = text.trim().trim_start_matches('#');
    let code = u32::from_str_radix(digits, 16).ok()?;
    (digits.len() == 6).then_some(Color(code))
}

impl SettingsDialog {
    /// Opens the editor on the color scheme `id`, or closes it when it is open on it.
    pub(super) fn edit_theme(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.themes.editing.as_ref().is_some_and(|e| e.id == id) {
            self.themes.editing = None;
        } else if let Some(theme) = self.themes.themes.iter().find(|theme| theme.id == id) {
            let mut editing = Editing {
                id: id.to_string(),
                file: theme.file.clone(),
                token: 0,
                slides: [0.; 3],
                saving: false,
                dirty: false,
            };
            editing.read_slides();
            self.themes.editing = Some(editing);
        }
        cx.notify();
    }

    /// Closes the editor when it is open on the color scheme `id`, which is gone or reset.
    pub(super) fn close_editor(&mut self, id: &str) {
        if self.themes.editing.as_ref().is_some_and(|e| e.id == id) {
            self.themes.editing = None;
        }
    }

    /// Names the color of the token being edited, or leaves it to be mixed, and writes the
    /// file.
    fn set_token(&mut self, color: Option<Color>, cx: &mut Context<Self>) {
        let Some(editing) = &mut self.themes.editing else {
            return;
        };
        (TOKENS[editing.token].set)(&mut editing.file, color);
        let (id, file) = (editing.id.clone(), editing.file.clone());
        // The card shows the change before the file is read again.
        if let Some(theme) = self.themes.themes.iter_mut().find(|theme| theme.id == id) {
            theme.notes = file.notes();
            theme.file = file;
        }
        self.save_edit(cx);
        cx.notify();
    }

    /// Writes the edited file, one write at a time: a change made during a write is written
    /// after it.
    fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = &mut self.themes.editing else {
            return;
        };
        if editing.saving {
            editing.dirty = true;
            return;
        }
        editing.saving = true;
        let (id, file) = (editing.id.clone(), editing.file.clone());
        let server = Arc::clone(&self.server);
        self.run(
            move || server.write_theme(&id, &file).map_err(|e| e.to_string()),
            |d, written, cx| {
                if let Err(e) = written {
                    d.themes.error = Some(e);
                }
                let Some(editing) = &mut d.themes.editing else {
                    return;
                };
                editing.saving = false;
                if std::mem::take(&mut editing.dirty) {
                    d.save_edit(cx);
                }
            },
            cx,
        );
    }
}

/// The colors to pick from, each a swatch over its name.
fn chips(
    token: usize,
    palette: &Colors,
    file: &ThemeFile,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let chips = TOKENS.iter().enumerate().map(|(n, entry)| {
        let checked = n == token;
        // A color the file leaves to be mixed is drawn fainter than one it names.
        let mixed = (entry.get)(file).is_none();
        div()
            .id(SharedString::from(format!("edit-token-{n}")))
            .role(Role::RadioButton)
            .aria_label(entry.name)
            .aria_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            })
            .tab_index(0)
            .focus_visible(focus_ring)
            .cursor_pointer()
            .flex_none()
            .h(px(26.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(radius_sm()))
            .border_1()
            .text_size(px(FS_SM))
            .map(|chip| {
                if checked {
                    chip.border_color(t.accent).bg(t.bg_accent_soft)
                } else {
                    chip.border_color(t.border).hover(move |s| s.bg(t.bg_hover))
                }
            })
            .when(mixed && !checked, |chip| chip.text_color(t.text_muted))
            .child(
                div()
                    .size(px(12.))
                    .rounded_full()
                    .border_1()
                    .border_color(t.border_strong)
                    .bg((entry.shown)(palette)),
            )
            .child(entry.name)
            .on_click(cx.listener(move |d, _, _, cx| {
                if let Some(editing) = &mut d.themes.editing {
                    editing.token = n;
                    editing.read_slides();
                }
                cx.notify();
            }))
    });
    div()
        .id("edit-tokens")
        .role(Role::RadioGroup)
        .aria_label("Color to edit")
        .py(px(8.))
        .flex()
        .flex_wrap()
        .gap(px(4.))
        .children(chips)
        .into_any_element()
}

/// The editor, under the color schemes, while one is being edited.
pub(super) fn editor(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Option<AnyElement> {
    let t = *theme(cx);
    let editing = d.themes.editing.as_ref()?;
    let (token, slides) = (editing.token, editing.slides);
    let entry = &TOKENS[token];
    let palette = Colors::from_file(&editing.file);
    let shown = (entry.shown)(&palette);
    let named = (entry.get)(&editing.file);
    let notes = editing.file.notes();
    let title = format!("Edit {}", editing.file.name);
    let picker = chips(token, &palette, &editing.file, cx);
    let Color(hex) = named.unwrap_or_else(|| code(shown));
    let field = d.text_field(
        "edit-hex",
        format!("{} as #rrggbb", entry.name),
        &format!("#{hex:06x}"),
        "#rrggbb",
        Some(100.),
        |d, text, cx| {
            let Some(color) = written(&text) else {
                return;
            };
            d.set_token(Some(color), cx);
            if let Some(editing) = &mut d.themes.editing {
                editing.read_slides();
            }
        },
        window,
        cx,
    );
    let mixed = (!entry.anchor && named.is_some()).then(|| {
        btn(
            "edit-mixed",
            "Use the mixed color",
            BtnKind::Plain,
            false,
            &t,
        )
        .child("Use the mixed color")
        .on_click(cx.listener(|d, _, _, cx| {
            d.set_token(None, cx);
            if let Some(editing) = &mut d.themes.editing {
                editing.read_slides();
            }
        }))
    });
    let color = token_row(entry.name, None, &t)
        .child(
            div()
                .flex_none()
                .size(px(24.))
                .rounded(px(radius_sm()))
                .border_1()
                .border_color(t.border_strong)
                .bg(rgb(hex)),
        )
        .child(field)
        .children(mixed);
    let mixers: Vec<_> = SLIDES
        .into_iter()
        .enumerate()
        .map(|(n, (name, max))| {
            let range = Range {
                min: 0.,
                max,
                step: 1.,
            };
            let slide = slider(
                SharedString::from(format!("edit-slide-{n}")),
                name,
                slides[n],
                range,
                &t,
                move |d, value, cx| {
                    let Some(editing) = &mut d.themes.editing else {
                        return;
                    };
                    editing.slides[n] = value;
                    let [hue, saturation, lightness] = editing.slides;
                    d.set_token(
                        Some(code(hsla(
                            hue / 360.,
                            saturation / 100.,
                            lightness / 100.,
                            1.,
                        ))),
                        cx,
                    );
                },
                cx,
            );
            token_row(name, None, &t).child(slide)
        })
        .collect();
    let done = btn("edit-done", "Done editing", BtnKind::Plain, false, &t)
        .child("Done")
        .on_click(cx.listener(|d, _, _, cx| {
            d.themes.editing = None;
            cx.notify();
        }));
    Some(
        div()
            .id("theme-editor")
            .role(Role::Group)
            .aria_label(title.clone())
            .mb(px(10.))
            .p(px(10.))
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(done),
            )
            .child(picker)
            .child(color)
            .children(mixers)
            .children(
                notes
                    .into_iter()
                    .enumerate()
                    .map(|(n, note)| alert(format!("edit-note-{n}"), note, t.warning)),
            )
            .into_any_element(),
    )
}
