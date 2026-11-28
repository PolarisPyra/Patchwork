//! What Codex says about the subagents and background tasks a turn starts.

// Rust guideline compliant 2026-10-09

use serde_json::Value;
use super::parse::codex_item;
use crate::events::ChatEvent;
use crate::chats::BackgroundTask;
use crate::chats::SubAgent;
use crate::chats::TaskStatus;
use crate::chats::ToolCall;
use crate::system::now_ms;
use crate::provider::Parsed;
use crate::provider::tool_started;
use crate::provider::ToolStart;
use crate::provider::TurnSpec;

/// The start of the id of a `chat/read` for a subagent's chat, which goes on with the
/// chat's id and the name the model gave the subagent.
pub(super) const SUBAGENT_READ: &str = "subagent";

/// The call that shows the subagent on `chat`, from the read of that chat, or `name` when
/// it could not be read.
pub(super) fn subagent_started(
    chat: &str,
    name: &str,
    read: &Value,
    spec: &TurnSpec,
    message_id: &str,
) -> Parsed {
    let found = &read["result"]["thread"];
    let kind = subagent_kind(found["agentRole"].as_str().unwrap_or(name), spec);
    let subagent = SubAgent {
        kind: kind.clone(),
        description: kind.clone(),
        model: found["model"].as_str().map(String::from),
        prompt: None,
    };
    tool_started(
        message_id,
        ToolStart {
            id: chat,
            name: "Agent",
            summary: kind,
            subagent: Some(subagent),
            parent: None,
        },
    )
}

/// The custom agent `name` is, when it is one, else `name` as written. Codex can write a role's
/// `-` as `_`, so a custom agent is found by either.
fn subagent_kind(name: &str, spec: &TurnSpec) -> String {
    spec.agents
        .iter()
        .find(|a| a.name == name || a.name.replace('-', "_") == name)
        .map_or_else(|| name.replace('_', "-"), |a| a.name.clone())
}

/// What a subagent's own frames add to its chat: its text, and the calls it makes.
#[must_use]
pub fn subagent_events(v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<ChatEvent> {
    let mut out = Vec::new();
    if let Some(kind @ ("item/started" | "item/completed")) = v["method"].as_str() {
        codex_item(
            kind == "item/completed",
            &v["params"]["item"],
            spec,
            message_id,
            &mut out,
        );
    }
    out.into_iter()
        .filter_map(|p| match p {
            Parsed::Event(event) => Some(event),
            _ => None,
        })
        .collect()
}

/// The background task a still-running Codex call becomes when its turn completes.
///
/// The background task an open Codex call becomes when its turn completes with the call still
/// running: a command, or a subagent chat. Codex has no request that stops one, so the task has
/// no id to stop it by and is listed without a stop button. Its only stop is the turn's own Stop,
/// which ends the process running it. Codex does not wake the agent when one ends either; the
/// agent sees the result only when it polls.
pub fn background_task(tool: &ToolCall) -> Option<BackgroundTask> {
    let kind = match tool.name.as_str() {
        "Shell" => "command",
        "Agent" if tool.subagent.is_some() => "agent",
        _ => return None,
    };
    Some(BackgroundTask {
        tool_id: tool.id.clone(),
        task_id: None,
        run_id: None,
        kind: kind.into(),
        description: tool.summary.clone(),
        status: TaskStatus::Running,
        started_at: tool.started_at.unwrap_or_else(now_ms),
        summary: None,
        output: None,
        cut_off: false,
    })
}
