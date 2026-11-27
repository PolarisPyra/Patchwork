//! Hook bundles: their hooks, the apps they name, and the events a hook runs on.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

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

/// An event a hook runs on, in the order a turn meets them. A manifest's keys match these whatever
/// their case, so Codex's `Stop` is `stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HookEvent {
    UserPromptSubmit,
    PreToolUse,
    Stop,
    Interrupt,
    SessionEnd,
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

impl HookEvent {
    pub const ALL: [HookEvent; 5] = [
        HookEvent::UserPromptSubmit,
        HookEvent::PreToolUse,
        HookEvent::Stop,
        HookEvent::Interrupt,
        HookEvent::SessionEnd,
    ];

    /// The event as Patchwork names it, and as `PATCHWORK_EVENT` gives it: `preToolUse`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            HookEvent::UserPromptSubmit => "userPromptSubmit",
            HookEvent::PreToolUse => "preToolUse",
            HookEvent::Stop => "stop",
            HookEvent::Interrupt => "interrupt",
            HookEvent::SessionEnd => "sessionEnd",
        }
    }

    /// The event as the agent CLIs name it: `PreToolUse`.
    #[must_use]
    pub fn cli_name(self) -> String {
        let name = self.name();
        name[..1].to_ascii_uppercase() + &name[1..]
    }

    /// The event a manifest names, whatever its case.
    #[must_use]
    pub fn parse(name: &str) -> Option<HookEvent> {
        HookEvent::ALL
            .into_iter()
            .find(|event| event.name().eq_ignore_ascii_case(name))
    }

    /// Whether Patchwork runs the event itself, for every agent, since only it knows how a reply
    /// ended. The rest happen inside the agent, so the agent runs them on its own hooks.
    #[must_use]
    pub fn runs_in_patchwork(self) -> bool {
        matches!(
            self,
            HookEvent::UserPromptSubmit
                | HookEvent::Stop
                | HookEvent::Interrupt
                | HookEvent::SessionEnd
        )
    }
}
