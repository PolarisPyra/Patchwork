//! The pane itself: the general rows, a tile per chat app, and the paired people, or the page of
//! the chat app that is open.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;

use gpui::{AnyElement, Window, div, prelude::*, px};
use patchwork_core::gateway::{Platform, Status};

use super::general::general;
use super::paired::paired;
use super::platform::page;
use crate::settings::SettingsDialog;
use crate::settings::controls::{alert, example, section_heading, tile, tile_grid, token_row};
use crate::ui::theme::theme;
use crate::ui::platform_logo;

/// How the personalities folder is laid out: one Markdown file per personality, named like it,
/// holding how it talks.
const PERSONALITIES_EXAMPLE: &str = "personalities/
├── Butler.md
└── Pirate.md

Pirate.md
Talk like a ship's first mate reporting to the captain: salty, cheerful
and short, with a little nautical slang (\"aye\", \"the hold\", \"smooth
sailing\"). Keep it readable: the slang never hides a file name, a
number or what is broken.";

/// One tile per chat app, with a dot for each one that is on; a click opens its settings.
fn tiles(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let gateway = &d.settings.gateway;
    let tiles = Platform::ALL.map(|platform| {
        let (id, label) = (platform.id(), platform.label());
        let enabled = gateway.platform(platform).enabled;
        let status = d
            .gateways
            .statuses
            .get(&platform)
            .cloned()
            .unwrap_or_default();
        let (state, dot) = match status {
            _ if !enabled => ("off", None),
            Status::Off => ("on", Some(t.border_strong)),
            Status::Connecting => ("connecting", Some(t.warning)),
            Status::Connected(_) => ("connected", Some(t.success)),
            Status::Error(_) => ("failed", Some(t.danger)),
        };
        let open = d.gateways.selected == Some(platform);
        tile(format!("gw-tile-{id}"), format!("{label}, {state}"), &t)
            .aria_expanded(open)
            .border_color(if open { t.accent } else { t.border })
            .bg(t.bg_panel)
            .text_color(t.text)
            .children(dot.map(|dot| {
                div()
                    .absolute()
                    .top(px(8.))
                    .right(px(8.))
                    .size(px(8.))
                    .rounded_full()
                    .bg(dot)
            }))
            .child(platform_logo(platform, 22.))
            .child(div().max_w_full().truncate().child(label))
            .on_click(cx.listener(move |d, _, _, cx| {
                d.gateways.selected = Some(platform);
                cx.notify();
            }))
    });
    tile_grid("Chat apps").children(tiles).into_any_element()
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    if d.gateways.saved.is_none() {
        d.gateways.saved = Some(HashSet::new());
        d.read_credentials(cx);
    }
    if !d.gateways.polling {
        d.poll_gateways(cx);
    }
    if let Some(each) = d.gateways.selected {
        return page(d, each, window, cx);
    }
    let rows = general(d, cx);
    let apps = tiles(d, cx);
    let people = paired(d, cx);
    div()
        .flex()
        .flex_col()
        .children(rows)
        .child(section_heading("Chat apps", false, &t))
        .child(apps)
        .child(section_heading("Paired apps", false, &t))
        .child(people)
        .children(
            d.gateways
                .error
                .clone()
                .map(|e| alert("gw-error", e, t.danger)),
        )
        .child(section_heading("Personalities", false, &t))
        .child(token_row("Folder", Some(&d.gateways.personalities_dir), &t))
        .child(example(PERSONALITIES_EXAMPLE, &t))
        .into_any_element()
}
