//! A server's row, with its client's logo.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, FontWeight, SharedString, div, img, prelude::*, px};
use patchwork_server::provider::AgentKind;
use patchwork_server::provider::mcp::ToolInfo;
use crate::settings::SettingsDialog;
use crate::settings::controls::letter_badge;
use crate::ui::theme::{FONT_MONO, FS_SM, FS_XS, RADIUS_SM, Theme};
use crate::ui::{agent_logo, icon};
use super::state::{Card, Client, dot};

/// A client's logo: the app icon for Patchwork, the agent's mark for a CLI.
pub(super) fn logo(client: Client, size: f32, app_icon: &SharedString) -> AnyElement {
    match client {
        Client::Patchwork => img(app_icon.clone())
            .size(px(size))
            .flex_none()
            .into_any_element(),
        Client::Claude => agent_logo(AgentKind::Claude, None, size),
        Client::Codex => agent_logo(AgentKind::Codex, None, size),
    }
}

/// A logo on a white disc, as the rows and the detail show who runs a server.
/// A one-colour logo is drawn black there: the disc stays white in OLED mode, so
/// the theme's text colour would paint it white on white.
pub(super) fn logo_disc(
    client: Client,
    size: f32,
    logo_size: f32,
    app_icon: &SharedString,
    t: &Theme,
) -> gpui::Div {
    div()
        .flex_none()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(t.border)
        .bg(gpui::white())
        .child(
            div()
                .text_color(gpui::black())
                .child(logo(client, logo_size, app_icon)),
        )
}

/// One server in the list: its name, status, command or matching tools, clients and tool count.
pub(super) fn server_row(
    card: &Card,
    matches: &[&ToolInfo],
    over: bool,
    app_icon: &SharedString,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let mut clients: Vec<Client> = Vec::new();
    for u in &card.uses {
        if !clients.contains(&u.client) {
            clients.push(u.client);
        }
    }
    let on = card.on_tools().len();
    let warn = over && card.own.as_ref().is_some_and(|o| o.status == "ready") && on > 0;
    let muted = t.text_muted;
    let tally = match card.status {
        "error" => div().text_color(t.danger).child("Error"),
        "starting" | "listing" => div().text_color(muted).child("…"),
        "unapproved" => div().text_color(muted).child("Not approved"),
        "signin" if card.tools.is_empty() => {
            let signed_in = card.uses.iter().any(|u| u.signed_in);
            div()
                .text_color(muted)
                .child(if signed_in { "Signed in" } else { "Signs in" })
        }
        _ => {
            let total = card.tools.len();
            let unit = if on < total {
                format!("of {total} tools")
            } else if on == 1 {
                "tool".into()
            } else {
                "tools".into()
            };
            div()
                .flex()
                .justify_end()
                .gap(px(4.))
                .child(
                    div()
                        .when(warn, |d| d.text_color(t.warning))
                        .child(on.to_string()),
                )
                .child(div().text_color(muted).child(unit))
        }
    };
    let id = card.id.clone();
    let hover = t.bg_hover;
    let title = if card.status == "error" {
        card.uses
            .iter()
            .find_map(|u| u.error.clone())
            .unwrap_or_default()
    } else {
        card.target.clone()
    };
    crate::ui::button(
        SharedString::from(format!("mcp-{}", card.id)),
        card.name.clone(),
    )
    .w_full()
    .min_w_0()
    .gap(px(10.))
    .px(px(12.))
    .py(px(8.))
    .text_color(t.text)
    .hover(move |s| s.bg(hover))
    .when(card.status == "off" || card.status == "unapproved", |b| {
        b.opacity(0.55)
    })
    .tooltip(crate::ui::tooltip(title))
    .child(letter_badge(&card.name, 24.))
    .child(
        div()
            .min_w_0()
            .flex_1()
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(card.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .size(px(6.))
                            .rounded_full()
                            .bg(dot(card.status, t)),
                    ),
            )
            .child(if matches.is_empty() {
                div()
                    .truncate()
                    .font_family(FONT_MONO)
                    .text_size(px(FS_XS))
                    .text_color(muted)
                    .child(card.target.clone())
            } else {
                div()
                    .flex()
                    .min_w_0()
                    .gap(px(4.))
                    .children(matches.iter().take(3).map(|tool| {
                        div()
                            .min_w_0()
                            .truncate()
                            .rounded(px(RADIUS_SM))
                            .bg(t.bg_accent_soft)
                            .px(px(6.))
                            .font_family(FONT_MONO)
                            .text_size(px(FS_XS))
                            .child(tool.name.clone())
                    }))
                    .when(matches.len() > 3, |d| {
                        d.child(
                            div()
                                .flex_none()
                                .text_size(px(FS_XS))
                                .text_color(muted)
                                .child(format!("+{}", matches.len() - 3)),
                        )
                    })
            }),
    )
    .child(
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .children(
                clients
                    .into_iter()
                    .map(|c| logo_disc(c, 24., 14., app_icon, t)),
            ),
    )
    .child(
        div()
            .w(px(112.))
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .text_size(px(FS_SM))
            .font_weight(FontWeight::SEMIBOLD)
            .child(tally),
    )
    .child(
        div()
            .flex_none()
            .text_color(muted)
            .child(icon("caret-right", 13.)),
    )
    .on_click(cx.listener(move |d, _, _, cx| {
        d.mcp.open_id = Some(id.clone());
        d.mcp.tool_query.clear();
        d.drop_fields("mcp-tools-filter");
        d.scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }))
    .into_any_element()
}
