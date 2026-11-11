//! The Local models pane.

// Rust guideline compliant 2026-10-09

use super::state::preset_of;
use super::state::from_discovered;
use super::state::health_line;
use super::state::trim_slash;
use gpui::{AnyElement, SharedString, Window, div, prelude::*, px};
use patchwork_server::provider::api::config::{DiscoveredServer, ProviderView};
use crate::settings::providers::pickers::{tile, tile_grid};
use crate::settings::SettingsDialog;
use crate::ui::theme::theme;
use crate::ui::provider_logo;

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let saved: Vec<ProviderView> = d
        .providers
        .providers
        .iter()
        .filter(|p| p.provider.local)
        .cloned()
        .collect();
    let saved_urls: Vec<&str> = saved
        .iter()
        .map(|p| trim_slash(&p.provider.base_url))
        .collect();
    let unsaved: Vec<&DiscoveredServer> = d
        .providers
        .local
        .found
        .iter()
        .flatten()
        .filter(|f| !saved_urls.contains(&trim_slash(&f.base_url)))
        .collect();
    let empty = d.providers.local.found.is_some() && saved.is_empty() && unsaved.is_empty();
    let selected = d.providers.local.selected.clone();

    let mut tiles: Vec<AnyElement> = Vec::new();
    for view in &saved {
        let p = &view.provider;
        let health = d.providers.local.health.get(&p.id);
        let (status, dot) = if !p.enabled {
            ("off", t.text_subtle)
        } else if let Some(h) = health {
            if h.reachable {
                ("answering", t.success)
            } else {
                ("not answering", t.danger)
            }
        } else {
            ("checking", t.border_strong)
        };
        let open = selected.as_deref() == Some(p.id.as_str());
        let id = p.id.clone();
        tiles.push(
            tile(format!("local-{id}"), format!("{}, {status}", p.name), &t)
                .aria_expanded(open)
                .border_color(if open { t.accent } else { t.border })
                .bg(t.bg_panel)
                .text_color(t.text)
                .tooltip(crate::ui::tooltip(health_line(p, health)))
                .child(
                    div()
                        .absolute()
                        .top(px(8.))
                        .right(px(8.))
                        .size(px(8.))
                        .rounded_full()
                        .bg(dot),
                )
                .child(provider_logo(&p.preset, p.logo.as_deref(), 22.))
                .child(div().max_w_full().truncate().child(p.name.clone()))
                .on_click(cx.listener(move |d, _, _, cx| {
                    let local = &mut d.providers.local;
                    local.selected = if local.selected.as_deref() == Some(id.as_str()) {
                        None
                    } else {
                        Some(id.clone())
                    };
                    local.repair_error = None;
                    d.close_address();
                    cx.notify();
                }))
                .into_any_element(),
        );
    }
    for found in &unsaved {
        let provider = from_discovered(found);
        tiles.push(
            tile(
                SharedString::from(format!("found-{}", found.base_url)),
                format!("Add {}", found.name),
                &t,
            )
            .border_color(t.border)
            .bg(t.bg_panel)
            .text_color(t.text)
            .tooltip(crate::ui::tooltip(found.base_url.clone()))
            .child(provider_logo(preset_of(found), None, 22.))
            .child(div().max_w_full().truncate().child(found.name.clone()))
            .on_click(cx.listener(move |d, _, window, cx| {
                d.open_form(provider.clone(), false, window, cx);
            }))
            .into_any_element(),
        );
    }
    let open = saved
        .iter()
        .find(|p| selected.as_deref() == Some(p.provider.id.as_str()))
        .cloned();
    let rows = open
        .map(|view| d.server_row(&view, window, cx))
        .unwrap_or_default();

    div()
        .flex()
        .flex_col()
        .when(!tiles.is_empty(), |c| {
            c.child(tile_grid("Local providers").children(tiles))
        })
        .children(rows)
        .when(empty, |c| {
            c.child(
                div()
                    .py(px(24.))
                    .flex()
                    .justify_center()
                    .text_color(t.text_muted)
                    .child("No local providers available"),
            )
        })
        .into_any_element()
}
