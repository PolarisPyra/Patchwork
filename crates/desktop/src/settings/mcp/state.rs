//! Settings → MCP's state: each server as a card, merged from Patchwork's and the CLIs' lists.

// Rust guideline compliant 2026-10-09

use gpui::Hsla;
use patchwork_server::provider::AgentKind;
use patchwork_server::provider::mcp::clis::CliGroup;
use patchwork_server::provider::mcp::{McpServer, ServerView, ToolInfo};
use crate::ui::theme::Theme;
use crate::ui::agent_label;
use super::form::Draft;

/// Who runs a server: Patchwork itself, or a CLI from its own config.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Client {
    Patchwork,
    Claude,
    Codex,
}

pub(super) const CLIENTS: [Client; 3] = [Client::Patchwork, Client::Claude, Client::Codex];

impl Client {
    pub(super) fn label(self) -> &'static str {
        match self {
            Client::Patchwork => "Patchwork",
            Client::Claude => agent_label(AgentKind::Claude),
            Client::Codex => agent_label(AgentKind::Codex),
        }
    }
}

/// Which status a server shared by several clients shows: the best one.
pub(super) const RANK: [&str; 7] = [
    "ready",
    "signin",
    "starting",
    "listing",
    "error",
    "unapproved",
    "off",
];

pub(super) fn rank(status: &str) -> usize {
    RANK.iter().position(|s| *s == status).unwrap_or(RANK.len())
}

/// A status's dot (`DOT`).
pub(super) fn dot(status: &str, t: &Theme) -> Hsla {
    match status {
        "ready" | "signin" => t.success,
        "starting" | "listing" => t.warning,
        "error" => t.danger,
        _ => t.text_subtle,
    }
}

/// Patchwork's own entry for a server: its name and its config, without secret values.
#[derive(Clone)]
pub(super) struct Own {
    pub(super) name: String,
    pub(super) server: McpServer,
    pub(super) status: &'static str,
}

/// One client's entry for a server.
#[derive(Clone)]
pub(super) struct Use {
    pub(super) client: Client,
    pub(super) name: String,
    pub(super) scope: Option<String>,
    pub(super) file: String,
    pub(super) status: &'static str,
    pub(super) error: Option<String>,
    pub(super) own: Option<Own>,
    /// A CLI's entry: its key, and whether its turns use Patchwork's copy.
    pub(super) cli: Option<(String, bool)>,
    /// The CLI holds its own sign-in to it.
    pub(super) signed_in: bool,
}

/// A server, once for every client that runs the same command or URL.
#[derive(Clone)]
pub(super) struct Card {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) target: String,
    pub(super) env: Vec<String>,
    pub(super) tools: Vec<ToolInfo>,
    pub(super) status: &'static str,
    pub(super) uses: Vec<Use>,
    /// Patchwork's own entry for it, whose tools can be turned off.
    pub(super) own: Option<Own>,
}

impl Card {
    /// The tools API turns get.
    pub(super) fn on_tools(&self) -> Vec<&ToolInfo> {
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
    pub(super) servers: Option<Vec<ServerView>>,
    pub(super) clis: Vec<CliGroup>,
    /// How many tools an API request takes.
    pub(super) budget: Option<usize>,
    pub(super) asked: bool,
    /// Whether the list is read again each second, while a server starts or lists its tools.
    pub(super) polling: bool,
    pub(super) editing: Option<Draft>,
    pub(super) filter: Option<Client>,
    pub(super) query: String,
    pub(super) open_id: Option<String>,
    pub(super) tool_query: String,
    pub(super) error: Option<String>,
}

pub(super) fn scope_label(scope: &str) -> &'static str {
    match scope {
        "user" => "User",
        "project" => "Project",
        _ => "Local",
    }
}

/// Patchwork's servers and the CLIs', one card per server: the same command, arguments and
/// environment, or URL and headers, wherever it is configured.
pub(super) fn merge(own: &[ServerView], clis: &[CliGroup]) -> Vec<Card> {
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
            card.own.clone_from(&u.own);
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
                signed_in: false,
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
                    signed_in: s.signed_in,
                },
            );
        }
    }
    cards
}

/// The command line or URL a server runs at.
pub(super) fn target(server: &McpServer) -> String {
    if let Some(url) = &server.url {
        return url.clone();
    }
    std::iter::once(server.command.clone().unwrap_or_default())
        .chain(server.args.iter().cloned())
        .map(|w| quote(&w))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn quote(word: &str) -> String {
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
