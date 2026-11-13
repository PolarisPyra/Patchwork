//! Which agents there are, what each one offers, and what an installed CLI reports about itself.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
    /// Saved as "gemini" before the rename; the alias keeps those threads and settings readable.
    #[serde(alias = "gemini")]
    Antigravity,
    OpenCode,
    /// OpenAI-compatible API providers, run by Patchwork's own agent.
    Api,
}

impl AgentKind {
    pub const ALL: [AgentKind; 5] = [
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::Antigravity,
        AgentKind::OpenCode,
        AgentKind::Api,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub kind: AgentKind,
    pub installed: bool,
    pub version: Option<String>,
    pub signed_in: bool,
    /// Human-readable one-liner, e.g. "Signed in with claude.ai" or "Not installed".
    pub detail: String,
}

/// A model the user can pick for an agent. `id: None` is the CLI's own default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: Option<String>,
    pub label: String,
    /// Configured CLI default; not a claim about a resumed session's runtime model.
    pub default_model: Option<String>,
    /// Superseded model, listed under the picker's "Legacy models" section.
    pub is_legacy: bool,
    /// The thinking levels this model takes, as its CLI reports them. `None` when the CLI does
    /// not say, so the picker offers the levels its agent shares; an empty list when the model
    /// takes none, so the picker offers no level for it.
    pub efforts: Option<Vec<String>>,
    /// How many tokens the model's context holds, as its CLI's catalog reports it. `None` when the
    /// catalog does not say, so the compact picker has nothing to measure "When full" against.
    pub context_window: Option<u64>,
}

/// A slash command advertised by a provider CLI for this thread/workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSlashCommand {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// A feature flag of the installed Codex CLI, from `codex features list`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexFeature {
    pub name: String,
    /// `stable`, `beta`, `under development`, `deprecated`, or `removed`.
    pub stage: String,
    pub enabled: bool,
}

/// A model in the installed Codex CLI's catalog, from `codex debug models`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModel {
    pub slug: String,
    pub display_name: String,
    pub context_window: Option<u64>,
    pub efforts: Vec<String>,
    pub speed_tiers: Vec<String>,
    pub multi_agent: Option<String>,
    /// Whether the CLI's own model picker shows it; one it hides is for internal use.
    pub listed: bool,
}

/// What the installed Codex CLI offers: its version, flags, and models.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexInfo {
    pub path: String,
    pub version: String,
    pub features: Vec<CodexFeature>,
    pub models: Vec<CodexModel>,
}

/// A feature gate of the installed Claude Code, as the CLI last cached it in `.claude.json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeFeature {
    /// The gate's name, such as `tengu_bridge_poll_interval_ms`.
    pub name: String,
    /// The cached value as one line of JSON, shortened when it is long.
    pub value: String,
    /// Present when the gate is a plain switch. A gate that carries a setting, such as a poll
    /// interval or a variant name, has no on or off, so this is `None`.
    pub enabled: Option<bool>,
}

/// A model in the catalog Claude Code cached for the terminal.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeModel {
    pub id: String,
    pub name: String,
    pub context_window: Option<u64>,
    pub efforts: Vec<String>,
    /// Whether the model offers fast mode.
    pub fast_mode: bool,
    /// Whether a newer model of the same line, such as a later Opus, replaces it.
    pub legacy: bool,
}

/// What the installed Claude Code offers: its path and version, the feature gates it has cached,
/// and the models in its catalog.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeInfo {
    pub path: String,
    pub version: String,
    pub features: Vec<ClaudeFeature>,
    pub models: Vec<ClaudeModel>,
}

/// A top-level setting from one of the config files OpenCode reads.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSetting {
    pub name: String,
    /// The value in short: a plain value as it is, an object as its keys, a list as its length.
    /// Values are not shown whole, since a provider's options can hold an API key.
    pub value: String,
    /// The config file that sets it.
    pub source: String,
}

/// What an installed OpenCode or Antigravity CLI offers: its path and version, its models, and
/// for OpenCode the settings its config files hold. Neither has feature flags.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInfo {
    pub path: String,
    pub version: String,
    pub settings: Vec<CliSetting>,
    pub models: Vec<ModelOption>,
}
