//! Claude Code's background tasks and subagents: their status, and a subagent's own frames read for its thread.

// Rust guideline compliant 2026-10-09

use serde_json::Value;
use crate::orchestration::AgentEvent;
use crate::system::now_ms;
use crate::threads::SubAgent;
use crate::threads::TaskStatus;
use crate::provider::Parsed;
use crate::provider::text_delta;
use crate::provider::tool_finished;
use super::{block_text, claude_tool_use};

/// What a subagent did in one of its frames (whole `assistant` and `user` messages that name the
/// call that started it): its text, reasoning, and calls, for its own thread.
pub fn subagent_events(v: &Value, root: &str, message_id: &str) -> Vec<AgentEvent> {
    let blocks = v["message"]["content"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let mut parsed = Vec::new();
    match v["type"].as_str() {
        Some("assistant") => {
            for block in blocks {
                match block["type"].as_str() {
                    Some("text") => {
                        let text = block["text"].as_str().unwrap_or_default();
                        if !text.trim().is_empty() {
                            parsed.push(text_delta(message_id, text));
                        }
                    }
                    Some("thinking") => {
                        let text = block["thinking"].as_str().unwrap_or_default();
                        if !text.trim().is_empty() {
                            let now = now_ms();
                            parsed.push(Parsed::Event(AgentEvent::Thinking {
                                message_id: message_id.into(),
                                text: text.into(),
                                started_at: Some(now),
                                ended_at: Some(now),
                            }));
                        }
                    }
                    Some("tool_use") => claude_tool_use(block, root, message_id, None, &mut parsed),
                    _ => {}
                }
            }
        }
        Some("user") => {
            parsed.extend(
                blocks
                    .iter()
                    .filter(|b| b["type"] == "tool_result")
                    .map(|block| tool_result(block, message_id)),
            );
        }
        _ => {}
    }
    parsed
        .into_iter()
        .filter_map(|p| match p {
            Parsed::Event(event) => Some(event),
            _ => None,
        })
        .collect()
}

/// A `tool_result` block: the call it answers is finished, with its output.
pub(super) fn tool_result(block: &Value, message_id: &str) -> Parsed {
    let id = block["tool_use_id"].as_str().unwrap_or_default();
    let is_error = block["is_error"].as_bool().unwrap_or(false);
    // Content is a string, or text blocks.
    let content = &block["content"];
    let output = content.as_str().map_or_else(
        || block_text(content.as_array().map_or(&[][..], Vec::as_slice)),
        String::from,
    );
    tool_finished(message_id, id, is_error, &output)
}

/// The task type of a shell command. Its call returns as soon as the command is started, so the
/// call is shown finished then, and the command is listed under the background until it ends.
pub const SHELL_TASK: &str = "local_bash";

/// A task's status as a task patch names it. A task that has not ended is running, pending, or
/// paused; anything else has ended.
pub(in crate::provider::claude) fn claude_task_status(status: &str) -> TaskStatus {
    match status {
        "running" => TaskStatus::Running,
        "pending" | "paused" => TaskStatus::Waiting,
        other => claude_ended_status(other),
    }
}

/// How an ended task is named: completed, failed, or killed (stopped by the user or its process).
pub(in crate::provider::claude) fn claude_ended_status(status: &str) -> TaskStatus {
    match status {
        "completed" => TaskStatus::Done,
        "failed" => TaskStatus::Failed,
        _ => TaskStatus::Stopped,
    }
}

/// Task calls carry a `subagent_type`; that is what starts a subagent.
pub(super) fn subagent_of(input: &Value) -> Option<SubAgent> {
    Some(SubAgent {
        kind: input["subagent_type"].as_str()?.into(),
        description: input["description"].as_str().unwrap_or_default().into(),
        model: input["model"].as_str().map(String::from),
        prompt: input["prompt"].as_str().map(String::from),
    })
}
