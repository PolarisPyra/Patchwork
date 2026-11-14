//! The cards that show how text reads: each code theme of Settings → Themes, each strength of
//! the chat background of Settings → Backgrounds, and each size of a chat's text and whether
//! anything moves of Settings → Interface.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use gpui::{AnyElement, Div, FontWeight, Hsla, ObjectFit, SharedString, div, img, prelude::*, px, rgb};
use patchwork_server::backgrounds::{BackgroundStrength, ChatBackground};
use patchwork_server::settings::{Settings, TextSize};
use patchwork_server::themes::Scheme;

use super::cards::{Extras, card, grid};
use super::files::{Filed, file_tools};
use crate::settings::SettingsDialog;
use crate::settings::controls::{switch, token_row};
use crate::ui::backdrop::{preset_fill, veil};
use crate::ui::theme::{FS_MD, FS_XS, Theme as Colors, font_mono, theme};

const STRENGTHS: [(BackgroundStrength, &str); 3] = [
    (BackgroundStrength::Subtle, "Subtle"),
    (BackgroundStrength::Medium, "Medium"),
    (BackgroundStrength::Strong, "Strong"),
];

const SIZES: [(TextSize, &str); 3] = [
    (TextSize::Small, "Small"),
    (TextSize::Medium, "Medium"),
    (TextSize::Large, "Large"),
];

/// The picture of a code theme on its card: a line of code in its colors.
fn code_swatch(colors: [Hsla; 4], t: &Colors) -> Div {
    let [keyword, function, string, comment] = colors;
    let part = |text: &'static str, color: Hsla| div().text_color(color).child(text);
    div()
        .bg(t.bg_editor)
        .px(px(10.))
        .flex()
        .flex_col()
        .justify_center()
        .font_family(font_mono())
        .text_size(px(FS_XS))
        .child(part("// hello", comment))
        .child(
            div()
                .flex()
                .gap(px(4.))
                .child(part("fn", keyword))
                .child(part("main", function)),
        )
        .child(part("\"text\"", string))
}

/// The code themes of light windows or of dark ones: the color scheme's own, then each file.
pub(super) fn code_theme_cards(
    d: &SettingsDialog,
    scheme: Scheme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let (key, title, current, set): (_, _, _, fn(&mut Settings, Option<String>)) = match scheme {
        Scheme::Light => (
            "code-theme-light",
            "Code in light windows",
            d.settings.code_theme_light.clone(),
            |s, id| s.code_theme_light = id,
        ),
        Scheme::Dark => (
            "code-theme-dark",
            "Code in dark windows",
            d.settings.code_theme_dark.clone(),
            |s, id| s.code_theme_dark = id,
        ),
    };
    let own = [t.syn_keyword, t.syn_function, t.syn_string, t.syn_comment];
    let mut cards = vec![card(
        format!("{key}-own"),
        "From the color scheme",
        code_swatch(own, &t),
        current.is_none(),
        Extras::default(),
        move |d, cx| d.update(|s| set(s, None), cx),
        cx,
    )];
    for code in d.themes.codes.iter().filter(|c| c.file.scheme == scheme) {
        let colors = code.file.colors;
        let inks = [
            colors.keyword,
            colors.function,
            colors.string,
            colors.comment,
        ]
        .map(|color| Hsla::from(rgb(color.0)));
        let id = code.id.clone();
        let card_key = format!("{key}-{id}");
        let state = (code.custom, code.edited);
        let tools = file_tools(&card_key, &code.file.name, Filed::Code, &id, state, cx);
        cards.push(card(
            card_key,
            code.file.name.clone(),
            code_swatch(inks, &t),
            current.as_deref() == Some(id.as_str()),
            Extras { notes: &[], tools },
            move |d, cx| {
                let id = id.clone();
                d.update(|s| set(s, Some(id)), cx);
            },
            cx,
        ));
    }
    titled(title, grid(key, title, cards, None), &t)
}

/// A titled grid of cards, as the chat background has.
pub(super) fn titled(title: &'static str, cards: AnyElement, t: &Colors) -> AnyElement {
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
