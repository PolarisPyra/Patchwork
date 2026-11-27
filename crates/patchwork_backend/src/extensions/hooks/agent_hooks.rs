//! The hooks an agent runs on its own events, such as before a tool call, and Patchwork's own agent
//! running the tool hooks around its calls. Each agent CLI gets them in the shape it reads: see
//! `provider::hooks`.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::process::run_hook_process;
use super::{HookEvent, HookHandler, list};
use crate::settings::Settings;

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

/// The hooks of the enabled bundles that the agent runs, rather than Patchwork.
#[must_use]
pub fn agent_hooks(settings: &Settings) -> Vec<AgentHook> {
    list(settings)
        .into_iter()
        .filter(|bundle| bundle.enabled)
        .flat_map(|bundle| bundle.hooks)
        .filter(|hook| !hook.event.runs_in_patchwork())
        .filter_map(|hook| {
            let (action, timeout_secs) = match hook.handler {
                HookHandler::Command {
                    command,
                    timeout_secs,
                } => (HookAction::Command { command }, timeout_secs),
                HookHandler::McpTool { .. }
                | HookHandler::Prompt
                | HookHandler::Agent
                | HookHandler::Unsupported { .. } => return None,
            };
            if let Some(matcher) = &hook.matcher
                && let Err(e) = regex::Regex::new(&format!("^(?:{matcher})$"))
            {
                log::warn!(
                    matcher:% = matcher, error:% = e;
                    "hook matcher {{matcher}} is not a valid pattern and matches no tool: {{error}}"
                );
            }
            Some(AgentHook {
                event: hook.event,
                matcher: hook.matcher,
                action,
                timeout_secs,
            })
        })
        .collect()
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
