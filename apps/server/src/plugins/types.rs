//! Plugin bundles: their hooks and the apps they name.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A plugin bundle: a folder with a `plugin.json`, the hooks it runs, and the apps it names.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    /// The folder name, and what `disabled_plugins` records.
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub path: String,
    pub hooks: Vec<PluginHook>,
    pub apps: Vec<PluginApp>,
    pub enabled: bool,
}

/// One handler a plugin runs on an event, such as `stop` or `preToolUse`.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PluginHook {
    pub event: HookEvent,
    /// The tools a tool event is limited to, as a regular expression over the tool's name.
    pub matcher: Option<String>,
    pub handler: PluginHookHandler,
}

/// An event a hook runs on, in the order a turn meets them. A manifest's keys match these whatever
/// their case, so Codex's `Stop` is `stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum HookEvent {
    UserPromptSubmit,
    SessionStart,
    PreToolUse,
    PermissionRequest,
    PostToolUse,
    PreCompact,
    PostCompact,
    SubagentStart,
    SubagentStop,
    Stop,
    Interrupt,
    SessionEnd,
}

/// What a hook does. `Command` and `Builtin` run; the rest are listed so the user sees them.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginHookHandler {
    Command {
        command: String,
        timeout_secs: u64,
    },
    /// A check Patchwork carries itself, by name, such as `boundedWaits`.
    Builtin {
        name: String,
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

/// A connector app a plugin asks for, as Codex lists them.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PluginApp {
    pub name: String,
    pub id: String,
    pub required: bool,
}
