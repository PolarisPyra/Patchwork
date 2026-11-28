//! Hook bundles: their hooks, the apps they name, and the events a hook runs on.

// Rust guideline compliant 2026-10-09

use serde::Serialize;

pub use patchwork_harness::hooks::HookEvent;

use crate::provider::AgentKind;

/// A bundle of hooks: a folder with a `plugin.json` or only a `hooks.json`, the hooks it runs, and
/// the apps it names.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bundle {
    /// The folder name, and what `disabled_hooks` records.
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub path: String,
    pub hooks: Vec<Hook>,
    pub apps: Vec<BundleApp>,
    pub enabled: bool,
}

/// One handler a bundle runs on an event, such as `stop` or `preToolUse`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hook {
    pub event: HookEvent,
    /// The tools a tool event is limited to, as a regular expression over the tool's name.
    pub matcher: Option<String>,
    pub handler: HookHandler,
    /// The agents whose turns run it. Empty when none does.
    pub run_by: Vec<AgentKind>,
}

/// What a hook does. `Command` runs; the rest are listed so the user sees them.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HookHandler {
    Command {
        command: String,
        timeout_secs: u64,
    },
    McpTool {
        server: String,
        tool: String,
    },
    Prompt,
    Agent,
    /// A handler type this version does not know.
    Unsupported {
        kind: String,
    },
}

/// A connector app a bundle asks for, as Codex lists them.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleApp {
    pub name: String,
    pub id: String,
    pub required: bool,
}
