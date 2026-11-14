//! The rows of Settings → Themes that set how code and chats read: the code themes, the chat
//! font, the size of a chat's text, and whether anything moves.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, App, ClickEvent, SharedString, prelude::*, px};
use patchwork_server::settings::{Settings, TextSize};
use patchwork_server::themes::Scheme;

use crate::settings::SettingsDialog;
use crate::settings::controls::{choice, segmented, select_box, switch, token_row};
use crate::ui::theme::theme;

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

/// The font chats are set in: the app's, the system's, or an installed reading font.
pub(super) fn font_row(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let fixed = [
        (None, SharedString::new_static("Default")),
        (
            Some(SYSTEM_FONT.to_string()),
            SharedString::new_static("System"),
        ),
    ];
    let options = fixed
        .into_iter()
        .chain(
            d.themes
                .fonts
                .iter()
                .map(|font| (Some(font.to_string()), font.clone())),
        )
        .collect();
    select_row(
        d,
        "chat-font",
        "Chat font",
        d.settings.chat_font.clone(),
        options,
        |s, font| s.chat_font = font,
        cx,
    )
}

/// How large the text of chats is.
pub(super) fn text_size_row(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    token_row("Chat text size", None, &t)
        .child(segmented(
            "text-size",
            "Chat text size",
            d.settings.text_size,
            &SIZES,
            &t,
            |d, size, cx| d.update(|s| s.text_size = size, cx),
            cx,
        ))
        .into_any_element()
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
