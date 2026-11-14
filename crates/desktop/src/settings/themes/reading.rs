//! The parts of Settings → Themes that set how code and chats read: the code themes, whether
//! anything moves, and a card for each strength of the chat background, each chat font and each
//! size of a chat's text.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use gpui::{
    AnyElement, App, ClickEvent, Div, FontWeight, ObjectFit, SharedString, div, img, prelude::*, px,
};
use patchwork_server::backgrounds::{BackgroundStrength, ChatBackground};
use patchwork_server::settings::{Settings, TextSize};
use patchwork_server::themes::Scheme;

use super::cards::{Extras, card, grid};
use crate::settings::SettingsDialog;
use crate::settings::controls::{choice, select_box, switch, token_row};
use crate::ui::backdrop::{preset_fill, veil};
use crate::ui::theme::{FS_MD, Theme as Colors, font_ui, theme};

const STRENGTHS: [(BackgroundStrength, &str); 3] = [
    (BackgroundStrength::Subtle, "Subtle"),
    (BackgroundStrength::Medium, "Medium"),
    (BackgroundStrength::Strong, "Strong"),
];

/// The family name that stands for the system's own font.
const SYSTEM_FONT: &str = ".SystemUIFont";

/// The fonts made to be easy to read, offered when they are installed.
const READING_FONTS: [&str; 4] = [
    "OpenDyslexic",
    "Atkinson Hyperlegible",
    "Atkinson Hyperlegible Next",
    "Lexend",
];

const SIZES: [(TextSize, &str); 3] = [
    (TextSize::Small, "Small"),
    (TextSize::Medium, "Medium"),
    (TextSize::Large, "Large"),
];

/// The reading fonts this computer has.
pub(super) fn installed_fonts(cx: &App) -> Vec<SharedString> {
    let installed = cx.text_system().all_font_names();
    READING_FONTS
        .into_iter()
        .filter(|font| installed.iter().any(|name| name == font))
        .map(SharedString::new_static)
        .collect()
}

/// A row with a select: what is picked, and a menu of `options` as `(value, name)`.
fn select_row(
    d: &SettingsDialog,
    key: &'static str,
    title: &'static str,
    current: Option<String>,
    options: Vec<(Option<String>, SharedString)>,
    set: fn(&mut Settings, Option<String>),
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let shown = options
        .iter()
        .find(|(value, _)| *value == current)
        .map(|(_, name)| name.clone())
        .or_else(|| current.clone().map(SharedString::from))
        .unwrap_or_default();
    let select = select_box(key, title, shown, d.menu_open(key), &t)
        .w(px(220.))
        .on_click(cx.listener(move |d, e: &ClickEvent, window, cx| {
            let weak = cx.entity().downgrade();
            let items = options
                .iter()
                .map(|(value, name)| {
                    let value = value.clone();
                    choice(
                        weak.clone(),
                        name.clone(),
                        value == current,
                        move |d, cx| {
                            let value = value.clone();
                            d.menu = None;
                            d.update(|s| set(s, value), cx);
                        },
                    )
                })
                .collect();
            d.toggle_menu(key, title, items, e.position(), window, cx);
        }));
    token_row(title, None, &t).child(select).into_any_element()
}

/// The code theme of light windows or of dark ones.
pub(super) fn code_theme_row(
    d: &SettingsDialog,
    scheme: Scheme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let own = (None, SharedString::new_static("From the color scheme"));
    let options = std::iter::once(own)
        .chain(
            d.themes
                .codes
                .iter()
                .filter(|code| code.file.scheme == scheme)
                .map(|code| {
                    (
                        Some(code.id.clone()),
                        SharedString::from(code.file.name.clone()),
                    )
                }),
        )
        .collect();
    match scheme {
        Scheme::Light => select_row(
            d,
            "code-theme-light",
            "Code in light windows",
            d.settings.code_theme_light.clone(),
            options,
            |s, id| s.code_theme_light = id,
            cx,
        ),
        Scheme::Dark => select_row(
            d,
            "code-theme-dark",
            "Code in dark windows",
            d.settings.code_theme_dark.clone(),
            options,
            |s, id| s.code_theme_dark = id,
            cx,
        ),
    }
}

/// A titled grid of cards, as the chat background has.
fn titled(title: &'static str, cards: AnyElement, t: &Colors) -> AnyElement {
    div()
        .border_b_1()
        .border_color(t.border)
        .pt(px(10.))
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
        .child(cards)
        .into_any_element()
}

/// The picture on a card that shows text: `sample` in `font` at `scale`, as a chat sets it.
fn text_swatch(font: SharedString, scale: f32, sample: &'static str, t: &Colors) -> Div {
    div()
        .bg(t.bg_editor)
        .px(px(10.))
        .flex()
        .flex_col()
        .justify_center()
        .font_family(font)
        .child(
            div()
                .text_size(px(20. * scale))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.text)
                .child("Aa"),
        )
        .child(
            div()
                .truncate()
                .text_size(px(FS_MD * scale))
                .text_color(t.text_muted)
                .child(sample),
        )
}

/// How much of the chat background shows: each strength over the background picked, or over
/// one of the app's own when none is.
pub(super) fn strength_cards(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let sample = if t.is_dark() { "dusk" } else { "sky" };
    let picture = match &d.settings.chat_background {
        ChatBackground::Image { file } => {
            let found = d.themes.pictures.iter().find(|p| &p.file == file);
            found.map(|p| PathBuf::from(p.path.clone()))
        }
        _ => None,
    };
    let preset = match &d.settings.chat_background {
        ChatBackground::Preset { id } => id.as_str(),
        _ => sample,
    };
    let cards = STRENGTHS
        .into_iter()
        .map(|(strength, name)| {
            let under = match &picture {
                Some(path) => div()
                    .absolute()
                    .inset_0()
                    .child(img(path.clone()).size_full().object_fit(ObjectFit::Cover)),
                None => preset_fill(preset, &t).unwrap_or_else(div),
            };
            let over = div()
                .absolute()
                .inset_0()
                .bg(t.bg_editor.opacity(veil(strength)))
                .px(px(10.))
                .flex()
                .items_center()
                .text_size(px(FS_MD))
                .text_color(t.text)
                .child("Aa");
            card(
                format!("strength-{name}"),
                name,
                div().relative().child(under).child(over),
                d.settings.chat_background_strength == strength,
                Extras {
                    notes: &[],
                    tools: Vec::new(),
                },
                move |d, cx| d.update(|s| s.chat_background_strength = strength, cx),
                cx,
            )
        })
        .collect();
    let cards = grid("strength", "Background strength", cards, None);
    titled("Background strength", cards, &t)
}

/// The font chats are set in: the app's, the system's, or an installed reading font.
pub(super) fn font_cards(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let fixed = [
        (None, SharedString::new_static("Default")),
        (
            Some(SYSTEM_FONT.to_string()),
            SharedString::new_static("System"),
        ),
    ];
    let fonts = d.themes.fonts.clone();
    let named = fonts.into_iter().map(|font| (Some(font.to_string()), font));
    let cards = fixed
        .into_iter()
        .chain(named)
        .map(|(value, name)| {
            let family = value.clone().map_or_else(font_ui, SharedString::from);
            let checked = d.settings.chat_font == value;
            card(
                format!("chat-font-{name}"),
                name,
                text_swatch(family, 1., "The quick brown fox", &t),
                checked,
                Extras {
                    notes: &[],
                    tools: Vec::new(),
                },
                move |d, cx| {
                    let value = value.clone();
                    d.update(|s| s.chat_font = value, cx);
                },
                cx,
            )
        })
        .collect();
    titled("Chat font", grid("chat-font", "Chat font", cards, None), &t)
}

/// How large the text of chats is.
pub(super) fn text_size_cards(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let font = crate::ui::theme::chat_font(cx);
    let cards = SIZES
        .into_iter()
        .map(|(size, name)| {
            card(
                format!("text-size-{name}"),
                name,
                text_swatch(font.clone(), size.scale(), "The quick brown fox", &t),
                d.settings.text_size == size,
                Extras {
                    notes: &[],
                    tools: Vec::new(),
                },
                move |d, cx| d.update(|s| s.text_size = size, cx),
                cx,
            )
        })
        .collect();
    let cards = grid("text-size", "Chat text size", cards, None);
    titled("Chat text size", cards, &t)
}

/// Whether animations and the Orchestrator's backdrop hold still.
pub(super) fn motion_row(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let on = d.settings.reduce_motion;
    token_row("Reduce motion", None, &t)
        .child(
            switch("reduce-motion", "Reduce motion", on, &t)
                .on_click(cx.listener(move |d, _, _, cx| d.update(|s| s.reduce_motion = !on, cx))),
        )
        .into_any_element()
}
