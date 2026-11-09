//! Settings → MCP Servers (`providers/mcp/`): Patchwork's own servers and the CLIs', one card
//! per server, opening to its page, and the form that adds or changes one of Patchwork's.

mod detail;
mod form;

use std::collections::BTreeMap;
use std::time::Duration;

use gpui::{AnyElement, Context, FontWeight, Hsla, SharedString, Window, div, img, prelude::*, px};
use patchwork_server::provider::AgentKind;
use patchwork_server::provider::mcp::clis::{self, CliGroup};
use patchwork_server::provider::mcp::{self, McpServer, ServerView, ToolInfo};

use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert, chip, letter_badge, list_box};
use crate::theme::{FONT_MONO, FS_SM, FS_XS, RADIUS, RADIUS_SM, Theme, theme};
use crate::ui::{BtnKind, agent_label, agent_logo, btn, icon};

pub use form::Draft;

/// Who runs a server: Patchwork itself, or a CLI from its own config.
#[derive(Clone, Copy, PartialEq)]
enum Client {
    Patchwork,
    Claude,
    Codex,
}

const CLIENTS: [Client; 3] = [Client::Patchwork, Client::Claude, Client::Codex];

impl Client {
    fn label(self) -> &'static str {
        match self {
            Client::Patchwork => "Patchwork",
            Client::Claude => agent_label(AgentKind::Claude),
            Client::Codex => agent_label(AgentKind::Codex),
        }
    }
}

/// Which status a server shared by several clients shows: the best one.
const RANK: [&str; 7] = [
    "ready",
    "signin",
    "starting",
    "listing",
    "error",
    "unapproved",
    "off",
];

fn rank(status: &str) -> usize {
    RANK.iter().position(|s| *s == status).unwrap_or(RANK.len())
}

/// A status's dot (`DOT`).
fn dot(status: &str, t: &Theme) -> Hsla {
    match status {
        "ready" | "signin" => t.success,
        "starting" | "listing" => t.warning,
        "error" => t.danger,
        _ => t.text_subtle,
    }
}

/// Patchwork's own entry for a server: its name and its config, without secret values.
#[derive(Clone)]
struct Own {
    name: String,
    server: McpServer,
    status: &'static str,
}

/// One client's entry for a server.
#[derive(Clone)]
struct Use {
    client: Client,
    name: String,
    scope: Option<String>,
    file: String,
    status: &'static str,
    error: Option<String>,
    own: Option<Own>,
    /// A CLI's entry: its key, and whether its turns use Patchwork's copy.
    cli: Option<(String, bool)>,
}

/// A server, once for every client that runs the same command or URL.
#[derive(Clone)]
struct Card {
    id: String,
    name: String,
    target: String,
    env: Vec<String>,
    tools: Vec<ToolInfo>,
    status: &'static str,
    uses: Vec<Use>,
    /// Patchwork's own entry for it, whose tools can be turned off.
    own: Option<Own>,
}

impl Card {
    /// The tools API turns get.
    fn on_tools(&self) -> Vec<&ToolInfo> {
        let off = self
            .own
            .as_ref()
            .map(|o| o.server.disabled_tools.as_slice())
            .unwrap_or_default();
        self.tools
            .iter()
            .filter(|t| !off.contains(&t.name))
            .collect()
    }
}

#[derive(Default)]
pub struct McpState {
    servers: Option<Vec<ServerView>>,
    clis: Vec<CliGroup>,
    /// How many tools an API request takes.
    budget: Option<usize>,
    asked: bool,
    /// Whether the list is read again each second, while a server starts or lists its tools.
    polling: bool,
    editing: Option<Draft>,
    filter: Option<Client>,
    query: String,
    open_id: Option<String>,
    tool_query: String,
    error: Option<String>,
}

fn scope_label(scope: &str) -> &'static str {
    match scope {
        "user" => "User",
        "project" => "Project",
        _ => "Local",
    }
}

/// Patchwork's servers and the CLIs', one card per server: the same command, arguments and
/// environment, or URL and headers, wherever it is configured.
fn merge(own: &[ServerView], clis: &[CliGroup]) -> Vec<Card> {
    let mut cards: Vec<Card> = Vec::new();
    let mut add = |id: &str, target: String, env: Vec<String>, tools: &[ToolInfo], u: Use| {
        let Some(card) = cards.iter_mut().find(|c| c.id == id) else {
            cards.push(Card {
                id: id.to_string(),
                name: u.name.clone(),
                target,
                env,
                tools: tools.to_vec(),
                status: u.status,
                own: u.own.clone(),
                uses: vec![u],
            });
            return;
        };
        if card.own.is_none() {
            card.own = u.own.clone();
        }
        if card.tools.is_empty() {
            card.tools = tools.to_vec();
        }
        if rank(u.status) < rank(card.status) {
            card.status = u.status;
        }
        card.uses.push(u);
    };
    for s in own {
        let env = if s.server.url.is_some() {
            &s.server.headers
        } else {
            &s.server.env
        };
        let own = Own {
            name: s.name.clone(),
            server: s.server.clone(),
            status: s.status,
        };
        add(
            &s.identity,
            target(&s.server),
            env.keys().cloned().collect(),
            &s.tools,
            Use {
                client: Client::Patchwork,
                name: s.name.clone(),
                scope: None,
                file: s.file.clone(),
                status: s.status,
                error: s.error.clone(),
                own: Some(own),
                cli: None,
            },
        );
    }
    for g in clis {
        let client = if g.agent == AgentKind::Codex {
            Client::Codex
        } else {
            Client::Claude
        };
        for s in &g.servers {
            let scope = scope_label(s.scope);
            let scope = match &s.project {
                Some(project) => format!("{scope} · {project}"),
                None => scope.to_string(),
            };
            let server = McpServer {
                command: s.command.clone(),
                args: s.args.clone(),
                url: s.url.clone(),
                ..Default::default()
            };
            add(
                &s.key,
                target(&server),
                s.env.clone(),
                &s.tools,
                Use {
                    client,
                    name: s.name.clone(),
                    scope: Some(scope),
                    file: s.file.clone(),
                    status: s.status,
                    error: s.error.clone(),
                    own: None,
                    cli: Some((s.key.clone(), s.relayed)),
                },
            );
        }
    }
    cards
}

/// The command line or URL a server runs at.
fn target(server: &McpServer) -> String {
    if let Some(url) = &server.url {
        return url.clone();
    }
    std::iter::once(server.command.clone().unwrap_or_default())
        .chain(server.args.iter().cloned())
        .map(|w| quote(&w))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || "@%+=:,./~-".contains(c));
    if plain {
        word.to_string()
    } else {
        format!("\"{}\"", word.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// `~/…` for a path under the home folder.
fn tildify(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path.starts_with(&home) => {
            format!("~{}", &path[home.len()..])
        }
        _ => path.to_string(),
    }
}

impl SettingsDialog {
    /// Reads Patchwork's servers and the CLIs' again, and follows them each second while one
    /// starts or lists its tools.
    fn reload_mcp(&mut self, cx: &mut Context<Self>) {
        let projects = self.settings.projects.clone();
        self.run(
            move || (mcp::list(), clis::list(&projects), mcp::tool_budget()),
            |d, (own, cli, budget), cx| {
                let busy = own.iter().any(|s| s.status == "starting")
                    || cli
                        .iter()
                        .any(|g| g.servers.iter().any(|s| s.status == "listing"));
                d.mcp.servers = Some(own);
                d.mcp.clis = cli;
                d.mcp.budget = Some(budget);
                if busy && !d.mcp.polling {
                    d.mcp.polling = true;
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(Duration::from_secs(1)).await;
                        let _ = this.update(cx, |d, cx| {
                            d.mcp.polling = false;
                            d.reload_mcp(cx);
                        });
                    })
                    .detach();
                }
            },
            cx,
        );
    }

    /// Runs an MCP change off the main thread, shows its error, and reads the servers again.
    fn mcp_run(
        &mut self,
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.mcp.error = None;
        self.run(
            work,
            |d, out, cx| {
                d.mcp.error = out.err();
                d.reload_mcp(cx);
            },
            cx,
        );
    }

    /// Opens a server's config file in the editor.
    fn open_mcp_config(&mut self, path: String, cx: &mut Context<Self>) {
        let projects = self.settings.projects.clone();
        self.mcp_run(
            move || {
                let path = std::path::PathBuf::from(path);
                if !clis::is_config_file(&path, &projects) {
                    return Err("That is not an MCP config file".into());
                }
                let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
                    return Err("That is not an MCP config file".into());
                };
                patchwork_server::workspace::open_in_editor(
                    &dir.to_string_lossy(),
                    &name.to_string_lossy(),
                    None,
                )
                .map_err(|e| e.to_string())
            },
            cx,
        );
    }
}

/// A client's logo: the app icon for Patchwork, the agent's mark for a CLI.
fn logo(client: Client, size: f32, app_icon: &SharedString) -> AnyElement {
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
fn logo_disc(
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
        .child(logo(client, logo_size, app_icon))
}

/// One server in the list: its name, status, command or matching tools, clients and tool count.
fn server_row(
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
        "signin" if card.tools.is_empty() => div().text_color(muted).child("Signs in"),
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
        .map(|e| alert("mcp-error", e, t.danger).mt(px(8.)));
    // Nothing to show until the servers are listed.
    let Some(servers) = &d.mcp.servers else {
        return div().children(error).into_any_element();
    };
    if d.mcp.editing.is_some() {
        return form::render(d, window, cx);
    }
    let cards = merge(servers, &d.mcp.clis);
    let app_icon = super::general::app_icon(&d.settings);
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

    let search = d
        .field(
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
        )
        .w_full()
        .pl(px(26.));
    let mut tools = div().flex().items_center().gap(px(6.)).pb(px(12.)).child(
        chip(
            "mcp-all",
            format!("All {}", cards.len()),
            filter.is_none(),
            &t,
        )
        .child("All")
        .child(
            div()
                .text_color(t.text_muted)
                .child(cards.len().to_string()),
        )
        .on_click(cx.listener(|d, _, _, cx| {
            d.mcp.filter = None;
            cx.notify();
        })),
    );
    for client in CLIENTS {
        let count = cards
            .iter()
            .filter(|c| c.uses.iter().any(|u| u.client == client))
            .count();
        tools = tools.child(
            chip(
                format!("mcp-{}", client.label()),
                format!("{} {count}", client.label()),
                filter == Some(client),
                &t,
            )
            .child(logo(client, 14., &app_icon))
            .child(client.label())
            .child(div().text_color(t.text_muted).child(count.to_string()))
            .on_click(cx.listener(move |d, _, _, cx| {
                d.mcp.filter = if d.mcp.filter == Some(client) {
                    None
                } else {
                    Some(client)
                };
                cx.notify();
            })),
        );
    }
    let muted = t.text_muted;
    tools = tools
        .child(
            div()
                .relative()
                .flex()
                .items_center()
                .ml_auto()
                .min_w_0()
                .max_w(px(208.))
                .flex_1()
                .child(search)
                .child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .text_color(muted)
                        .child(icon("magnifying-glass", 13.)),
                ),
        )
        .child(
            btn("mcp-add", "Add server", BtnKind::Plain, false, &t)
                .px(px(10.))
                .child(icon("plus", 13.))
                .child("Add server")
                .on_click(
                    cx.listener(|d, _, window, cx| d.open_mcp_form(Draft::default(), window, cx)),
                ),
        );

    let warning = over.then(|| {
        div()
            .mb(px(12.))
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.warning.opacity(0.4))
            .bg(t.warning.opacity(0.1))
            .px(px(12.))
            .py(px(8.))
            .text_size(px(FS_SM))
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
        div()
            .py(px(24.))
            .flex()
            .justify_center()
            .text_color(muted)
            .child(if query.is_empty() {
                "No servers".to_string()
            } else {
                format!("Nothing matches “{}”", typed.trim())
            })
    } else {
        list_box(rows, &t)
    };
    div()
        .flex()
        .flex_col()
        .child(tools)
        .children(warning)
        .child(list)
        .children(error)
        .into_any_element()
}

/// `KEY=value` lines for environment variables, `Name: value` lines for headers, with a stored
/// value shown as `STORED`.
fn pairs_text(pairs: &BTreeMap<String, String>, sep: &str) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}{sep}{}", if v.is_empty() { form::STORED } else { v }))
        .collect::<Vec<_>>()
        .join("\n")
}
