//! The cards of Settings → Themes: a color scheme or a backdrop as a picture over its name.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{
    AnyElement, Div, FontWeight, Hsla, ObjectFit, Role, SharedString, Toggled, div, img,
    prelude::*, px, rgb,
};
use patchwork_server::settings::{Settings, Theme as ThemeSetting};
use patchwork_server::themes::{BackdropChoice, Dim, Fit, Palette, Scheme};

use super::Kind;
use crate::orchestrator::{butterfly_preview, scene_preview};
use crate::settings::SettingsDialog;
use crate::ui::theme::{FS_LG, FS_SM, radius, radius_sm, Theme as Colors, theme};
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
                .text_size(px(FS_LG))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(text)
                .child("Aa"),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(FS_SM))
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
fn tool(
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

/// The button on a card that deletes its file.
fn delete_tool(
    key: &str,
    name: &str,
    kind: Kind,
    id: String,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    tool(
        format!("delete-{key}"),
        format!("Delete {name}"),
        "trash",
        move |d, cx| d.remove_themed(kind, id.clone(), cx),
        cx,
    )
}

/// What a card shows beside its name: what is wrong with it, and its buttons.
#[derive(Default)]
struct Extras<'a> {
    notes: &'a [String],
    tools: Vec<AnyElement>,
}

/// A card to pick in one of the grids: a picture over a name.
fn card(
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
                .text_size(px(FS_SM))
                .child(div().flex_1().min_w_0().truncate().child(name))
                .children(warning)
                .children(tools),
        )
        .on_click(cx.listener(move |d, _, _, cx| pick(d, cx)))
        .into_any_element()
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
        let mut tools = vec![tool(
            format!("export-{key}"),
            format!("Export {name}"),
            "export",
            {
                let id = id.clone();
                move |d, cx| d.export_theme(id.clone(), cx)
            },
            cx,
        )];
        if scheme.custom {
            tools.push(delete_tool(&key, &name, Kind::Scheme, id.clone(), cx));
        }
        // Picked as the one color scheme, it brings the background it names.
        let backdrop = scheme
            .file
            .backdrop_choice()
            .filter(|_| slot == Slot::Fixed);
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
                            s.orchestrator_backdrop = backdrop;
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

/// The card after the backdrops that adds a picture to them, the size of a card's picture.
pub(super) fn add_picture_card(cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    button("add-backdrop", "Add background")
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
        .text_size(px(FS_SM))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(t.bg_hover).text_color(t.text))
        .child(icon("plus", 16.))
        .child("Add background")
        .on_click(cx.listener(|d, _, _, cx| d.add_backdrop_picture(cx)))
        .into_any_element()
}

/// The Orchestrator's backdrops: none, the butterfly, the animated backgrounds, the pictures.
pub(super) fn backdrop_cards(
    d: &SettingsDialog,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let t = *theme(cx);
    let current = d.settings.orchestrator_backdrop.clone();
    let still = d.settings.reduce_motion;
    let butterfly = div().child(butterfly_preview(&d.themes.flight, still));
    let fixed = [
        ("None", div().bg(t.bg_app), BackdropChoice::None),
        ("Butterfly", butterfly, BackdropChoice::Butterfly),
    ];
    let scenes = d.themes.scenes.iter().map(|saved| {
        let id = saved.id.clone();
        (
            saved.file.name.clone(),
            div().child(scene_preview(
                Arc::clone(saved),
                t.bg_app,
                t.text_muted,
                still,
            )),
            BackdropChoice::Scene { id: id.clone() },
            saved.custom.then_some((Kind::Scene, id)),
        )
    });
    let pictures = d.themes.pictures.iter().map(|picture| {
        // The picked picture keeps how it is shown, and its card shows it that way.
        let choice = if current.picture_file() == Some(picture.file.as_str()) {
            current.clone()
        } else {
            BackdropChoice::picture(picture.file.clone())
        };
        let (dim, fit) = match &choice {
            BackdropChoice::Image { dim, fit, .. } => (*dim, *fit),
            _ => (Dim::default(), Fit::default()),
        };
        let shown = img(picture.path.clone())
            .size_full()
            .object_fit(object_fit(fit));
        (
            picture.name.clone().unwrap_or_else(|| "Picture".into()),
            // Dimmed toward the window's color, as the page shows it.
            div()
                .relative()
                .bg(t.bg_app)
                .child(shown)
                .child(div().absolute().inset_0().bg(t.bg_app.opacity(dim.share()))),
            choice,
            None,
        )
    });
    let all: Vec<_> = fixed
        .into_iter()
        .map(|(name, swatch, choice)| (name.to_string(), swatch, choice, None))
        .chain(scenes)
        .chain(pictures)
        .collect();
    all.into_iter()
        .enumerate()
        .map(|(n, (name, swatch, choice, delete))| {
            let checked = choice == current;
            let key = format!("backdrop-{n}");
            let tools = delete
                .map(|(kind, id)| delete_tool(&key, &name, kind, id, cx))
                .into_iter()
                .collect();
            card(
                key,
                name,
                swatch,
                checked,
                Extras { notes: &[], tools },
                move |d, cx| {
                    let choice = choice.clone();
                    d.update(|s| s.orchestrator_backdrop = choice, cx);
                },
                cx,
            )
        })
        .collect()
}

/// How a picture is laid on the page for each way it can fill it.
fn object_fit(fit: Fit) -> ObjectFit {
    match fit {
        Fit::Cover => ObjectFit::Cover,
        Fit::Contain => ObjectFit::Contain,
        Fit::Stretch => ObjectFit::Fill,
    }
}
