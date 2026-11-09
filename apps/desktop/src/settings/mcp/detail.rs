//! A server's page (`ServerDetail.tsx`): who runs it and from which file, its status, its
//! environment's names, and its tools, which Patchwork's own copy can leave out of API turns.

use gpui::{AnyElement, Context, FontWeight, SharedString, Toggled, Window, div, prelude::*, px};
use patchwork_server::provider::mcp::{self, McpServer, clis};

use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, letter_badge, switch};
use crate::ui::theme::{FONT_MONO, FS_LG, FS_SM, FS_XS, RADIUS, RADIUS_SM, Theme, theme};
use crate::ui::{BtnKind, btn, icon_button};

use super::{Card, Client, Draft, Own, tildify};
use super::rows::{logo_disc};

/// The field the tools are filtered with.
const TOOLS_FILTER: &str = "mcp-tools-filter";

impl SettingsDialog {
    /// Saves Patchwork's own server `own` as `change` leaves its config.
    fn save_own(&mut self, own: &Own, change: impl FnOnce(&mut McpServer), cx: &mut Context<Self>) {
        let mut server = own.server.clone();
        change(&mut server);
        let name = own.name.clone();
        self.mcp_run(move || mcp::save(name.clone(), Some(name), server), cx);
    }
}

/// One client's entry: its logo and status, its file, its scope, and what can be done with it.
fn use_row(
    u: &super::Use,
    app_icon: &SharedString,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let client = u.client.label();
    let file = u.file.clone();
    let (fg, muted) = (t.text, t.text_muted);
    let status = u.status;
    let line = match &u.error {
        Some(e) => div()
            .truncate()
            .text_size(px(FS_SM))
            .text_color(t.danger)
            .child(e.clone()),
        None => div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(px(6.))
            .text_size(px(FS_XS))
            .text_color(muted)
            .child(
                crate::ui::button(
                    SharedString::from(format!("mcp-file-{client}-{}", u.name)),
                    "Open in editor",
                )
                .min_w_0()
                .truncate()
                .font_family(FONT_MONO)
                .text_color(muted)
                .hover(move |s| s.text_color(fg).underline())
                .tooltip(crate::ui::tooltip("Open in editor"))
                .child(tildify(&u.file))
                .on_click(cx.listener(move |d, _, _, cx| d.open_mcp_config(file.clone(), cx))),
            )
            .when(u.cli.as_ref().is_some_and(|(_, relayed)| *relayed), |d| {
                d.child(div().flex_none().child("· runs Patchwork's copy"))
            })
            .when(status == "unapproved", |d| {
                d.child(div().flex_none().child(if u.client == Client::Codex {
                    "· project not trusted in Codex"
                } else {
                    "· not approved in Claude Code"
                }))
            }),
    };
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(t.border)
        .px(px(12.))
        .py(px(10.))
        .child(
            div()
                .relative()
                .flex_none()
                .child(logo_disc(u.client, 32., 16., app_icon, t))
                .child(
                    div()
                        .absolute()
                        .right(px(-2.))
                        .bottom(px(-2.))
                        .size(px(10.))
                        .rounded_full()
                        .border_2()
                        .border_color(t.bg_elevated)
                        .bg(super::dot(status, t)),
                ),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(div().font_weight(FontWeight::MEDIUM).child(client))
                .child(line),
        )
        .children(u.scope.clone().map(|scope| {
            div()
                .flex_none()
                .rounded(px(RADIUS_SM))
                .bg(t.bg_hover)
                .px(px(6.))
                .text_size(px(FS_XS))
                .text_color(muted)
                .child(scope)
        }));
    if let Some(own) = u.own.clone() {
        let name = u.name.clone();
        if status == "error" || status == "ready" {
            let restart = name.clone();
            row = row.child(
                icon_button(
                    SharedString::from(format!("mcp-restart-{name}")),
                    format!("Restart {name} for {client}"),
                    "arrow-clockwise",
                    15.,
                    cx,
                )
                .tooltip(crate::ui::tooltip("Restart"))
                .on_click(cx.listener(move |d, _, _, cx| {
                    let name = restart.clone();
                    d.mcp_run(
                        move || {
                            mcp::restart(&name);
                            Ok(())
                        },
                        cx,
                    )
                })),
            );
        }
        let edit = own.clone();
        row = row
            .child(
                btn(
                    SharedString::from(format!("mcp-edit-{name}")),
                    format!("Edit {name}"),
                    BtnKind::Plain,
                    false,
                    t,
                )
                .child("Edit")
                .on_click(cx.listener(move |d, _, window, cx| {
                    let draft = Draft::edit(
                        Some(edit.name.clone()),
                        edit.name.clone(),
                        edit.server.clone(),
                    );
                    d.open_mcp_form(draft, window, cx)
                })),
            )
            .child(
                switch(
                    SharedString::from(format!("mcp-use-{name}")),
                    format!("Use {name}"),
                    status != "off",
                    t,
                )
                .on_click(cx.listener(move |d, _, _, cx| {
                    d.save_own(&own, |s| s.disabled = !s.disabled, cx)
                })),
            );
    }
    if let Some((key, _)) = u.cli.clone() {
        row = if status != "off" && status != "listing" && status != "unapproved" {
            row.child(
                icon_button(
                    SharedString::from(format!("mcp-refresh-{key}")),
                    "List tools again",
                    "arrow-clockwise",
                    15.,
                    cx,
                )
                .tooltip(crate::ui::tooltip("List tools again"))
                .on_click(cx.listener(move |d, _, _, cx| {
                    let (key, projects) = (key.clone(), d.settings.projects.clone());
                    d.mcp_run(
                        move || {
                            clis::refresh(&key, &projects);
                            Ok(())
                        },
                        cx,
                    )
                })),
            )
        } else {
            // Keeps the scope in line with the rows that have the button.
            row.child(div().flex_none().size(px(28.)))
        };
    }
    row.into_any_element()
}

pub fn render(
    d: &mut SettingsDialog,
    card: &Card,
    app_icon: &SharedString,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let typed = d.mcp.tool_query.clone();
    let query = typed.trim().to_lowercase();
    let tools: Vec<_> = card
        .tools
        .iter()
        .filter(|tool| {
            query.is_empty()
                || format!("{} {}", tool.name, tool.description)
                    .to_lowercase()
                    .contains(&query)
        })
        .cloned()
        .collect();
    let own = card.own.clone();
    let off: Vec<String> = own
        .as_ref()
        .map(|o| o.server.disabled_tools.clone())
        .unwrap_or_default();
    // A CLI's server Patchwork can run too, unless it signs in through the CLI.
    let enable = if card.uses.iter().any(|u| u.client == Client::Patchwork) {
        None
    } else {
        card.uses
            .iter()
            .find(|u| u.cli.is_some() && u.status != "signin" && u.status != "unapproved")
            .and_then(|u| u.cli.as_ref().map(|(key, _)| key.clone()))
    };

    let header = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            icon_button("mcp-back", "Back", "caret-left", 16., cx)
                .tooltip(crate::ui::tooltip("Back"))
                .on_click(cx.listener(|d, _, _, cx| {
                    d.mcp.open_id = None;
                    cx.notify();
                })),
        )
        .child(letter_badge(&card.name, 40.))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .id("mcp-detail-title")
                        .role(gpui::Role::Heading)
                        .text_size(px(FS_LG))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(card.name.clone()),
                )
                .child(
                    div()
                        .font_family(FONT_MONO)
                        .text_size(px(FS_SM))
                        .text_color(t.text_muted)
                        .child(card.target.clone()),
                ),
        )
        .children(enable.map(|key| {
            btn(
                "mcp-enable",
                "Enable in Patchwork",
                BtnKind::Primary,
                false,
                &t,
            )
            .child("Enable in Patchwork")
            .on_click(cx.listener(move |d, _, _, cx| {
                let (key, projects) = (key.clone(), d.settings.projects.clone());
                d.mcp_run(move || clis::enable(&key, &projects), cx)
            }))
        }));

    let uses: Vec<AnyElement> = card
        .uses
        .iter()
        .map(|u| use_row(u, app_icon, &t, cx))
        .collect();

    // Claude Code and Codex can use Patchwork's copy of a server they also configure.
    let share = own
        .clone()
        .filter(|o| o.server.command.is_some() && card.uses.iter().any(|u| u.cli.is_some()))
        .map(|o| {
            let mut names: Vec<&str> = Vec::new();
            for u in card.uses.iter().filter(|u| u.cli.is_some()) {
                if !names.contains(&u.client.label()) {
                    names.push(u.client.label());
                }
            }
            let on = o.server.share;
            let label = format!("{} use Patchwork's copy", names.join(" and "));
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .rounded(px(RADIUS))
                .border_1()
                .border_color(t.border)
                .px(px(12.))
                .py(px(10.))
                .child(label.clone())
                .child(switch("mcp-share", label, on, &t).on_click(
                    cx.listener(move |d, _, _, cx| d.save_own(&o, |s| s.share = !s.share, cx)),
                ))
        });

    let env = (!card.env.is_empty()).then(|| {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .children(card.env.iter().map(|e| {
                div()
                    .rounded(px(RADIUS_SM))
                    .bg(t.bg_hover)
                    .px(px(6.))
                    .font_family(FONT_MONO)
                    .text_size(px(FS_XS))
                    .text_color(t.text_muted)
                    .child(e.clone())
            }))
    });

    let filter = (card.tools.len() > 8).then(|| {
        d.field(
            TOOLS_FILTER,
            "Filter tools",
            &typed,
            "Filter tools",
            FieldKind {
                live: true,
                ..Default::default()
            },
            |d, text, cx| {
                d.mcp.tool_query = text;
                cx.notify();
            },
            window,
            cx,
        )
        .ml_auto()
        .w(px(208.))
        .flex_none()
    });
    let tools_section = (!card.tools.is_empty()).then(|| {
        let count = match &own {
            Some(_) => format!("{} of {} tools on", card.on_tools().len(), card.tools.len()),
            None => format!("{} tools", card.tools.len()),
        };
        let names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
        let turn = |on: bool, own: Own, off: Vec<String>, names: Vec<String>| {
            move |d: &mut SettingsDialog,
                  _: &gpui::ClickEvent,
                  _: &mut Window,
                  cx: &mut Context<SettingsDialog>| {
                let mut next = off.clone();
                next.retain(|n| !names.contains(n));
                if !on {
                    next.extend(names.iter().cloned());
                }
                d.save_own(&own, |s| s.disabled_tools = next, cx);
            }
        };
        let tiles = tools.iter().map(|tool| {
            let body = |d: gpui::Stateful<gpui::Div>| {
                d.min_w_0()
                    .overflow_hidden()
                    .rounded(px(RADIUS_SM))
                    .border_1()
                    .px(px(10.))
                    .py(px(6.))
                    .tooltip(crate::ui::tooltip(tool.description.clone()))
                    .child(
                        div()
                            .truncate()
                            .font_family(FONT_MONO)
                            .text_size(px(FS_SM))
                            .child(tool.name.clone()),
                    )
                    .child(
                        div()
                            .w_full()
                            .line_clamp(2)
                            .text_ellipsis()
                            .text_size(px(FS_XS))
                            .text_color(t.text_muted)
                            .child(tool.description.clone()),
                    )
            };
            let id = SharedString::from(format!("mcp-tool-{}", tool.name));
            match &own {
                None => body(div().id(id)).border_color(t.border).into_any_element(),
                Some(o) => {
                    let is_on = !off.contains(&tool.name);
                    let (o, name, off) = (o.clone(), tool.name.clone(), off.clone());
                    let (accent, faded) = (t.accent, t.accent.opacity(0.6));
                    body(div().id(id))
                        .role(gpui::Role::Switch)
                        .aria_label(SharedString::from(tool.name.clone()))
                        .aria_toggled(if is_on { Toggled::True } else { Toggled::False })
                        .tab_index(0)
                        .focus_visible(crate::ui::focus_ring)
                        .cursor_pointer()
                        .map(|b| {
                            if is_on {
                                b.border_color(faded).hover(move |s| s.border_color(accent))
                            } else {
                                b.border_color(t.border)
                                    .opacity(0.5)
                                    .hover(|s| s.opacity(0.8))
                            }
                        })
                        .on_click(cx.listener(move |d, _, _, cx| {
                            let mut next = off.clone();
                            if is_on {
                                next.push(name.clone());
                            } else {
                                next.retain(|n| *n != name);
                            }
                            d.save_own(&o, |s| s.disabled_tools = next, cx);
                        }))
                        .into_any_element()
                }
            }
        });
        let tiles: Vec<AnyElement> = tiles.collect();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(count),
                    )
                    .children(own.clone().map(|o| {
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                btn("mcp-all-on", "All on", BtnKind::Plain, false, &t)
                                    .child("All on")
                                    .on_click(cx.listener(turn(
                                        true,
                                        o.clone(),
                                        off.clone(),
                                        names.clone(),
                                    ))),
                            )
                            .child(
                                btn("mcp-all-off", "All off", BtnKind::Plain, false, &t)
                                    .child("All off")
                                    .on_click(cx.listener(turn(
                                        false,
                                        o,
                                        off.clone(),
                                        names.clone(),
                                    ))),
                            )
                    }))
                    .children(filter),
            )
            .child(div().grid().grid_cols(2).gap(px(6.)).children(tiles))
    });

    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(header)
        .child(div().flex().flex_col().gap(px(6.)).children(uses))
        .children(share)
        .children(env)
        .children(tools_section)
        .into_any_element()
}
