//! Settings → MCP drawn.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;
use gpui::{AnyElement, Window, div, prelude::*, px};
use patchwork_client::local::core::provider::mcp::ToolInfo;
use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, Section, alert, empty, list_box, search_box};
use crate::settings::extensions::filter_select;
use patchwork_design_icons::IconName;
use patchwork_design_theme::{fs_sm, radius, theme};
use patchwork_design_components::{BtnKind, btn};
use patchwork_design_icons::icon;
use super::form::Draft;
use super::rows::server_row;
use super::servers::{CLIENTS, Card, merge};
use super::detail;
use super::form;

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    if !std::mem::replace(&mut d.mcp.asked, true) {
        d.reload_mcp(cx);
    }
    let t = *theme(cx);
    let error = d
        .mcp
        .error
        .clone()
        .map(|e| alert("mcp-error", e, t.danger).pt_0().pb(px(12.)));
    // Nothing to show until the servers are listed.
    let Some(servers) = &d.mcp.servers else {
        return div().children(error).into_any_element();
    };
    if d.mcp.editing.is_some() {
        return form::render(d, window, cx);
    }
    let cards = merge(servers, &d.mcp.clis);
    let app_icon = crate::settings::general::app_icon(&d.settings);
    // Tools API turns would get; past the budget, the last ones are left out.
    let on_count: usize = cards
        .iter()
        .filter(|c| c.own.as_ref().is_some_and(|o| o.status == "ready"))
        .map(|c| c.on_tools().len())
        .sum();
    let budget = d.mcp.budget.unwrap_or(usize::MAX);
    let over = on_count > budget;

    if let Some(card) = d
        .mcp
        .open_id
        .as_ref()
        .and_then(|id| cards.iter().find(|c| &c.id == id))
        .cloned()
    {
        return div()
            .flex()
            .flex_col()
            .child(detail::render(d, &card, &app_icon, window, cx))
            .children(error)
            .into_any_element();
    }

    let filter = d.mcp.filter;
    let typed = d.mcp.query.clone();
    let query = typed.trim().to_lowercase();
    let matches = |c: &Card| -> Vec<ToolInfo> {
        if query.is_empty() {
            return Vec::new();
        }
        c.tools
            .iter()
            .filter(|t| {
                format!("{} {}", t.name, t.description)
                    .to_lowercase()
                    .contains(&query)
            })
            .cloned()
            .collect()
    };
    let shown: Vec<(&Card, Vec<ToolInfo>)> = cards
        .iter()
        .filter(|c| filter.is_none_or(|f| c.uses.iter().any(|u| u.client == f)))
        .map(|c| (c, matches(c)))
        .filter(|(c, m)| {
            query.is_empty() || c.name.to_lowercase().contains(&query) || !m.is_empty()
        })
        .collect();

    let search = d.field(
        "mcp-search",
        "Search MCP servers",
        &typed,
        "Search",
        FieldKind {
            live: true,
            ..Default::default()
        },
        |d, text, cx| {
            d.mcp.query = text;
            cx.notify();
        },
        window,
        cx,
    );
    let mut options = vec![(None, "All", cards.len())];
    for client in CLIENTS {
        let count = cards
            .iter()
            .filter(|c| c.uses.iter().any(|u| u.client == client))
            .count();
        options.push((Some(client), client.label(), count));
    }
    let picker = filter_select(
        d,
        "mcp-filter",
        "Show servers for",
        options,
        filter,
        |d, chosen, cx| {
            d.mcp.filter = chosen;
            cx.notify();
        },
        cx,
    );
    let tools = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(picker)
        .child(search_box(search, &t).min_w_0().w(px(208.)))
        .child(
            btn("mcp-add", "Add server", BtnKind::Plain, false, &t)
                .px(px(10.))
                .gap(px(6.))
                .child(icon(IconName::Plus, 13.))
                .child("Add server")
                .on_click(
                    cx.listener(|d, _, window, cx| d.open_mcp_form(Draft::default(), window, cx)),
                ),
        );

    let warning = over.then(|| {
        div()
            .mb(px(12.))
            .rounded(px(radius()))
            .border_1()
            .border_color(t.warning.opacity(0.4))
            .bg(t.warning.opacity(0.1))
            .px(px(12.))
            .py(px(8.))
            .text_size(px(fs_sm()))
            .child(format!(
                "{on_count} tools are on, and an API request takes {budget}. The last {} are left out; turn some off on a server's page.",
                on_count - budget
            ))
    });
    let rows: Vec<AnyElement> = shown
        .iter()
        .map(|(c, m)| server_row(c, &m.iter().collect::<Vec<_>>(), over, &app_icon, &t, cx))
        .collect();
    let list = if rows.is_empty() {
        empty(
            if query.is_empty() {
                "No servers".to_string()
            } else {
                format!("Nothing matches “{}”", typed.trim())
            },
            &t,
        )
    } else {
        list_box(rows, &t)
    };
    div()
        .flex()
        .flex_col()
        .children(error)
        .child(
            Section::new("MCP servers")
                .action(tools)
                .bare()
                .children(warning)
                .child(list),
        )
        .into_any_element()
}

/// `KEY=value` lines for environment variables, `Name: value` lines for headers, with a stored
/// value shown as `STORED`.
pub(super) fn pairs_text(pairs: &BTreeMap<String, String>, sep: &str) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}{sep}{}", if v.is_empty() { form::STORED } else { v }))
        .collect::<Vec<_>>()
        .join("\n")
}
