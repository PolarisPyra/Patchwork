//! The hooks an agent runs on its own events, such as before a tool call: what they are, and
//! running one.

// Rust guideline compliant 2026-11-27

mod process;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use process::{Finished, run_hook_process};

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

/// A hook an agent runs on one of its own events. The agent gives it the event as JSON on stdin
/// and reads its answer, so a tool hook can refuse a call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHook {
    pub event: HookEvent,
    pub matcher: Option<String>,
    pub action: HookAction,
    pub timeout_secs: u64,
}

/// What a hook runs: a shell command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HookAction {
    Command { command: String },
}

impl AgentHook {
    /// Whether the hook runs for a call to `tool`. A matcher is a regular expression over the whole
    /// tool name, so `Bash` is that tool alone and `Edit|Write` either.
    #[must_use]
    pub fn matches(&self, tool: &str) -> bool {
        self.matcher.as_deref().is_none_or(|matcher| {
            regex::Regex::new(&format!("^(?:{matcher})$")).is_ok_and(|re| re.is_match(tool))
        })
    }
}

/// Runs a tool hook for Patchwork's own agent, returning the refusal reason if any.
///
/// Runs a tool hook for Patchwork's own agent the way Claude Code runs one, and returns the reason
/// the call is refused, or None to let it run. A command gets
/// `input` as JSON on stdin, and exit code 2 with a reason on stderr, or a `deny` decision on
/// stdout, refuses the call; one that fails or runs past its timeout lets the call run, as in the
/// CLIs. `root` is the project folder as Patchwork keeps it, so a hook of a project on another
/// machine runs there.
#[must_use]
pub fn run_tool_hook(hook: &AgentHook, input: &Value, root: &str) -> Option<String> {
    let HookAction::Command { command } = &hook.action;
    run_command(hook, command, input, root)
}

fn run_command(hook: &AgentHook, command: &str, input: &Value, root: &str) -> Option<String> {
    let event = hook.event.name();
    let done = run_hook_process(
        root,
        command,
        &[],
        Some(input.to_string()),
        hook.timeout_secs,
    )
    .inspect_err(|e| log::warn!(event:%, error:% = e; "hook {{event}}: {{error}}"))
    .ok()?;
    let (stdout, stderr) = (done.stdout, done.stderr);
    match done.status.code() {
        Some(2) => Some(
            Some(stderr.trim())
                .filter(|reason| !reason.is_empty())
                .unwrap_or("A hook refused this call")
                .to_string(),
        ),
        Some(0) => {
            let answer: Value = serde_json::from_str(stdout.trim()).unwrap_or_default();
            let output = &answer["hookSpecificOutput"];
            let reason = |key: &str, value: &Value| {
                value[key]
                    .as_str()
                    .filter(|reason| !reason.is_empty())
                    .unwrap_or("A hook refused this call")
                    .to_string()
            };
            if output["permissionDecision"] == "deny" {
                Some(reason("permissionDecisionReason", output))
            } else if answer["decision"] == "block" {
                Some(reason("reason", &answer))
            } else {
                None
            }
        }
        _ => {
            log::warn!(event:%, stderr = stderr.trim(); "hook {{event}} failed: {{stderr}}");
            None
        }
    }
}
