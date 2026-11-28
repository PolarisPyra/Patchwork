//! The hooks an agent runs on its own events, such as before a tool call, and Patchwork's own agent
//! running the tool hooks around its calls. Each agent CLI gets them in the shape it reads: see
//! `provider::hooks`.

// Rust guideline compliant 2026-10-09

pub use patchwork_harness::hooks::{AgentHook, HookAction, run_tool_hook};
use super::{HookHandler, list};
use crate::settings::Settings;

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
