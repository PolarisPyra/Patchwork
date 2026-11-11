//! What a CLI offers, read from the installed program: its version, feature flags and models.

// Rust guideline compliant 2026-10-09

use patchwork_server::provider::cli_info::InfoCommand;
use patchwork_server::provider::{AgentKind, ModelOption, antigravity, claude, codex, opencode};

/// The field the rows are searched with.
pub(super) const SEARCH_FIELD: &str = "cli-search";

/// One flag or setting of a CLI. `kept` is whether the filter chip keeps it; `source` is the file
/// that sets it.
pub(super) struct Feature {
    pub(super) name: String,
    pub(super) detail: String,
    pub(super) enabled: Option<bool>,
    pub(super) kept: bool,
    pub(super) source: Option<String>,
}

pub(super) struct Model {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) context_window: Option<u64>,
    pub(super) efforts: Vec<String>,
    pub(super) tag: Option<String>,
}

/// Each CLI's reading in one shape, so one view draws them all.
pub(in crate::settings::providers) struct Info {
    pub(super) path: String,
    pub(super) version: String,
    pub(super) features: Vec<Feature>,
    pub(super) models: Vec<Model>,
}

/// What a CLI's rows are.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Rows {
    Flags,
    Settings,
}

/// How each CLI is read and what its page offers (`CLIS`).
pub(super) struct Cli {
    /// The binary's name, which titles a command's output.
    pub(super) name: &'static str,
    /// Each command: its button, its output's title, and what it runs.
    pub(super) commands: &'static [(&'static str, &'static str, InfoCommand)],
    pub(super) rows: Option<Rows>,
    pub(super) filter: Option<&'static str>,
    pub(super) empty: &'static str,
}

/// The commands Claude Code and Codex share.
pub(super) const SHARED: &[(&str, &str, InfoCommand)] = &[
    ("Doctor", "doctor", InfoCommand::Doctor),
    ("MCP servers", "mcp list", InfoCommand::McpList),
    ("Plugins", "plugin list", InfoCommand::PluginList),
];

pub(super) fn cli(kind: AgentKind) -> Option<Cli> {
    Some(match kind {
        AgentKind::Claude => Cli {
            name: "claude",
            commands: SHARED,
            rows: Some(Rows::Flags),
            filter: Some("On only"),
            empty: "Claude Code has not cached its feature gates yet. Start it once, then read again.",
        },
        AgentKind::Codex => Cli {
            name: "codex",
            commands: SHARED,
            rows: Some(Rows::Flags),
            filter: Some("Stable only"),
            empty: "Nothing matches.",
        },
        AgentKind::OpenCode => Cli {
            name: "opencode",
            commands: &[
                ("Agents", "debug agents", InfoCommand::Agents),
                ("Paths", "debug paths", InfoCommand::Paths),
                ("MCP servers", "mcp list", InfoCommand::McpList),
                ("Plugins", "plugin list", InfoCommand::PluginList),
            ],
            rows: Some(Rows::Settings),
            filter: None,
            empty: "No config file sets anything yet.",
        },
        AgentKind::Antigravity => Cli {
            name: "agy",
            commands: &[
                ("Changelog", "changelog", InfoCommand::Changelog),
                ("MCP servers", "mcp list", InfoCommand::McpList),
                ("Plugins", "plugin list", InfoCommand::PluginList),
            ],
            rows: None,
            filter: None,
            empty: "Nothing matches.",
        },
        AgentKind::Api => return None,
    })
}

/// Whether Patchwork can read `kind`'s details, so its row offers them.
pub fn has_details(kind: AgentKind) -> bool {
    cli(kind).is_some()
}

/// OpenCode's and Antigravity's models, which carry no tag.
pub(super) fn cli_models(models: Vec<ModelOption>) -> Vec<Model> {
    models
        .into_iter()
        .map(|m| Model {
            id: m.id.unwrap_or_else(|| m.label.clone()),
            name: m.label,
            context_window: m.context_window,
            efforts: m.efforts.unwrap_or_default(),
            tag: None,
        })
        .collect()
}

/// Reads the CLI. Claude Code's gates and catalog come from its caches, Codex's from the binary,
/// OpenCode's rows are the settings in its config files, and Antigravity shows none.
pub(super) fn read(kind: AgentKind) -> Result<Info, patchwork_server::Error> {
    match kind {
        AgentKind::Claude => claude::cli::info().map(|i| Info {
            path: i.path,
            version: i.version,
            features: i
                .features
                .into_iter()
                .map(|f| Feature {
                    kept: f.enabled == Some(true),
                    name: f.name,
                    detail: f.value,
                    enabled: f.enabled,
                    source: None,
                })
                .collect(),
            models: i
                .models
                .into_iter()
                .map(|m| Model {
                    id: m.id,
                    name: m.name,
                    context_window: m.context_window,
                    efforts: m.efforts,
                    tag: m.fast_mode.then(|| "fast".to_string()),
                })
                .collect(),
        }),
        AgentKind::Codex => codex::cli::info().map(|i| Info {
            path: i.path,
            version: i.version,
            features: i
                .features
                .into_iter()
                .map(|f| Feature {
                    kept: f.stage == "stable",
                    name: f.name,
                    detail: f.stage,
                    enabled: Some(f.enabled),
                    source: None,
                })
                .collect(),
            models: i
                .models
                .into_iter()
                .map(|m| Model {
                    id: m.slug,
                    name: m.display_name,
                    context_window: m.context_window,
                    efforts: m.efforts,
                    tag: m.multi_agent,
                })
                .collect(),
        }),
        AgentKind::OpenCode => opencode::cli::info().map(|i| Info {
            path: i.path,
            version: i.version,
            features: i
                .settings
                .into_iter()
                .map(|s| Feature {
                    name: s.name,
                    detail: s.value,
                    enabled: None,
                    kept: true,
                    source: Some(s.source),
                })
                .collect(),
            models: cli_models(i.models),
        }),
        AgentKind::Antigravity => antigravity::info().map(|i| Info {
            path: i.path,
            version: i.version,
            features: Vec::new(),
            models: cli_models(i.models),
        }),
        AgentKind::Api => Err("API providers have no CLI".into()),
    }
}

pub(super) fn run_command(
    kind: AgentKind,
    command: InfoCommand,
) -> Result<String, patchwork_server::Error> {
    match kind {
        AgentKind::Claude => claude::cli::command(command),
        AgentKind::Codex => codex::cli::command(command),
        AgentKind::OpenCode => opencode::cli::command(command),
        AgentKind::Antigravity => antigravity::command(command),
        AgentKind::Api => Err("API providers have no CLI".into()),
    }
}

/// The open Details page.
pub struct Details {
    pub(super) kind: AgentKind,
    pub(super) info: Option<Info>,
    pub(super) error: Option<String>,
    pub(super) query: String,
    pub(super) filtered: bool,
    /// The last command's title and output.
    pub(super) output: Option<(String, String)>,
    pub(super) busy: bool,
}
