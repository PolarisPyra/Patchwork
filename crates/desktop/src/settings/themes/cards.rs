//! The cards of Settings → Themes: a color scheme or a backdrop as a picture over its name.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, Div, FontWeight, Hsla, Role, SharedString, Stateful, Toggled, div, prelude::*, px,
    rgb,
};
use patchwork_server::settings::{Settings, Theme as ThemeSetting};
use patchwork_server::themes::{Palette, Scheme};

use super::files::{Filed, file_tools};
use crate::settings::SettingsDialog;
use crate::ui::theme::{fs_lg, fs_sm, Theme as Colors, radius, radius_sm, theme};
use crate::ui::{button, focus_ring, icon, icon_button, tooltip};

/// The color schemes that are not files, in the order their cards come: the id each has in
/// the settings, its name, the kind of window it is for, and its colors.
const BUILT_IN: [(&str, &str, Scheme, Palette); 3] = [
    ("light", "Light", Scheme::Light, Palette::Light),
    ("dark", "Dark", Scheme::Dark, Palette::Dark),
    ("oled", "OLED", Scheme::Dark, Palette::Oled),
];

/// Which setting a grid of color schemes picks for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Slot {
    /// The one color scheme the app is drawn in.
    Fixed,
    /// The color scheme drawn while the system is light.
    Light,
    /// The color scheme drawn while the system is dark.
    Dark,
}

impl Slot {
    fn key(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// The kind of window whose background a color scheme picked here brings its own to.
    fn windows(self) -> Option<Scheme> {
        match self {
            Self::Fixed => None,
            Self::Light => Some(Scheme::Light),
            Self::Dark => Some(Scheme::Dark),
        }
    }

    /// Whether a color scheme for `scheme` windows belongs in this grid.
    fn takes(self, scheme: Scheme) -> bool {
        match self {
            Self::Fixed => true,
            Self::Light => scheme == Scheme::Light,
            Self::Dark => scheme == Scheme::Dark,
        }
    }

    /// The id picked in `settings`, and whether it is a file's.
    fn picked(self, settings: &Settings) -> (String, bool) {
        let built_in = |id: &str| (id.to_string(), false);
        match self {
            Self::Fixed => match (&settings.theme_id, settings.theme) {
                (Some(id), _) => (id.clone(), true),
                (None, ThemeSetting::Light) => built_in("light"),
                (None, ThemeSetting::Oled) => built_in("oled"),
                (None, ThemeSetting::Dark | ThemeSetting::System) => built_in("dark"),
            },
            Self::Light => settings
                .system_light
                .clone()
                .map_or_else(|| built_in("light"), |id| (id, true)),
            Self::Dark => match settings.system_dark.as_deref() {
                None => built_in("dark"),
                Some("oled") => built_in("oled"),
                Some(id) => (id.to_string(), true),
            },
        }
    }

    /// Picks the color scheme with `id`, a file's or a built-in one.
    fn pick(self, settings: &mut Settings, id: &str, file: bool) {
        match self {
            Self::Fixed if file => settings.theme_id = Some(id.to_string()),
            Self::Fixed => {
                settings.theme_id = None;
                settings.theme = match id {
                    "light" => ThemeSetting::Light,
                    "oled" => ThemeSetting::Oled,
                    _ => ThemeSetting::Dark,
                };
            }
            Self::Light => settings.system_light = file.then(|| id.to_string()),
            Self::Dark => {
                settings.system_dark = (file || id == "oled").then(|| id.to_string());
            }
        }
    }
}

/// The picture of a color scheme on its card: its surface, both texts and its accent.
fn colors_swatch(surface: Hsla, text: Hsla, muted: Hsla, accent: Hsla) -> Div {
    div()
        .bg(surface)
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(fs_lg()))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(text)
                .child("Aa"),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(fs_sm()))
                .text_color(muted)
                .child("Aa"),
        )
        .child(div().size(px(14.)).rounded_full().bg(accent))
}

/// The picture of a built-in color scheme.
fn palette_swatch(palette: &Colors) -> Div {
    colors_swatch(
        palette.bg_app,
        palette.text,
        palette.text_muted,
        palette.accent,
    )
}

/// A small button on a card, beside its name.
pub(super) fn tool(
    key: String,
    label: String,
    icon: &'static str,
    act: impl Fn(&mut SettingsDialog, &mut Context<SettingsDialog>) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    icon_button(SharedString::from(key), label, icon, 14., cx)
        .size(px(22.))
        .flex_none()
        .on_click(cx.listener(move |d, _, _, cx| {
            cx.stop_propagation();
            act(d, cx);
        }))
        .into_any_element()
}

/// What a card shows beside its name: what is wrong with it, and its buttons.
#[derive(Default)]
pub(super) struct Extras<'a> {
    pub(super) notes: &'a [String],
    pub(super) tools: Vec<AnyElement>,
}

/// A card to pick in one of the grids: a picture over a name.
pub(super) fn card(
    key: String,
    name: impl Into<SharedString>,
    swatch: Div,
    checked: bool,
    extras: Extras<'_>,
    pick: impl Fn(&mut SettingsDialog, &mut Context<SettingsDialog>) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let name: SharedString = name.into();
    let Extras { notes, tools } = extras;
    let warning = (!notes.is_empty()).then(|| {
        let notes = notes.join(". ");
        div()
            .id(SharedString::from(format!("notes-{key}")))
            .flex_none()
            .text_color(t.warning)
            .child(icon("warning", 14.))
            .tooltip(tooltip(notes))
    });
    // The card's name says what is wrong with it, for a reader that cannot hover.
    let label = std::iter::once(name.to_string())
        .chain(notes.iter().cloned())
        .collect::<Vec<_>>()
        .join(". ");
    div()
        .id(SharedString::from(key))
        .role(Role::RadioButton)
        .aria_label(label)
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .tab_index(0)
        .focus_visible(focus_ring)
        .cursor_pointer()
        .w(px(150.))
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(4.))
        .p(px(6.))
        .rounded(px(radius()))
        .border_1()
        .map(|card| {
            if checked {
                card.border_color(t.accent).bg(t.bg_accent_soft)
            } else {
                card.border_color(gpui::transparent_black())
                    .hover(move |s| s.bg(t.bg_hover))
            }
        })
        .child(
            swatch
                .h(px(64.))
                .rounded(px(radius_sm()))
                .border_1()
                .border_color(t.border)
                .overflow_hidden(),
        )
        .child(
            div()
                .h(px(22.))
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(fs_sm()))
                .child(div().flex_1().min_w_0().truncate().child(name))
                .children(warning)
                .children(tools),
        )
        .on_click(cx.listener(move |d, _, _, cx| pick(d, cx)))
        .into_any_element()
}

/// The card after a grid's last one that adds to the grid, the size of a card's picture.
pub(super) fn add_card(id: &'static str, label: &'static str, t: &Colors) -> Stateful<Div> {
    let t = *t;
    button(id, label)
        // The size and place of a card's picture: inside the card's padding and border.
        .m(px(7.))
        .w(px(136.))
        .h(px(64.))
        .flex_none()
        .flex_col()
        .justify_center()
        .gap(px(6.))
        .rounded(px(radius_sm()))
        .border_1()
        .border_dashed()
        .border_color(t.border_strong)
        .text_size(px(fs_sm()))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(t.bg_hover).text_color(t.text))
        .child(icon("plus", 16.))
        .child(label)
}

/// A grid of cards, of which one is picked.
///
/// `after` comes right after its last card, in the same row, and is not one of the choices.
pub(super) fn grid(
    id: &'static str,
    label: &'static str,
    cards: Vec<AnyElement>,
    after: Option<AnyElement>,
) -> AnyElement {
    let group = div()
        .id(id)
        .role(Role::RadioGroup)
        .aria_label(label)
        .flex()
        .flex_wrap()
        .gap(px(4.))
        .children(cards)
        .children(after);
    div().py(px(10.)).child(group).into_any_element()
}

/// The color schemes of a grid: the built-in ones, then the files.
pub(super) fn scheme_cards(
    d: &SettingsDialog,
    slot: Slot,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let (picked, picked_file) = slot.picked(&d.settings);
    let mut cards: Vec<AnyElement> = BUILT_IN
        .iter()
        .filter(|(_, _, scheme, _)| slot.takes(*scheme))
        .map(|(id, name, _, palette)| {
            let id = *id;
            card(
                format!("scheme-{}-built-in-{id}", slot.key()),
                *name,
                palette_swatch(&Colors::of(palette)),
                !picked_file && picked == id,
                Extras::default(),
                move |d, cx| d.update(|s| slot.pick(s, id, false), cx),
                cx,
            )
        })
        .collect();
    for scheme in &d.themes.themes {
        if !slot.takes(scheme.file.scheme) {
            continue;
        }
        let colors = scheme.file.colors;
        let id = scheme.id.clone();
        let key = format!("scheme-{}-{id}", slot.key());
        let name = scheme.file.name.clone();
        let mut tools = vec![
            tool(
                format!("edit-{key}"),
                format!("Edit {name}"),
                "pencil-simple",
                {
                    let id = id.clone();
                    move |d, cx| d.edit_theme(&id, cx)
                },
                cx,
            ),
            tool(
                format!("export-{key}"),
                format!("Export {name}"),
                "export",
                {
                    let id = id.clone();
                    move |d, cx| d.export_theme(id.clone(), cx)
                },
                cx,
            ),
        ];
        let state = (scheme.custom, scheme.edited);
        tools.extend(file_tools(&key, &name, Filed::Scheme, &id, state, cx));
        // It brings the background it names to the windows it is picked for.
        let backdrop = scheme.file.backdrop_choice();
        cards.push(card(
            key,
            name,
            colors_swatch(
                rgb(colors.surface.0).into(),
                rgb(colors.text.0).into(),
                rgb(colors.muted.0).into(),
                rgb(colors.accent.0).into(),
            ),
            picked_file && picked == id,
            Extras {
                notes: &scheme.notes,
                tools,
            },
            move |d, cx| {
                let backdrop = backdrop.clone();
                d.update(
                    |s| {
                        slot.pick(s, &id, true);
                        if let Some(backdrop) = backdrop {
                            *s.backdrop_mut(slot.windows()) = backdrop;
                        }
                    },
                    cx,
                );
            },
            cx,
        ));
    }
    cards
}
