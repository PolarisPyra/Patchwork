//! Routes each line from `agy acp` by kind, and tells work and background commands apart.

// Rust guideline compliant 2026-11-23

use serde_json::Value;

use crate::chats::{BackgroundTask, TaskStatus, ToolCall};
use crate::provider::{Parsed, TurnSpec};
use crate::system::now_ms;

use super::permission_prompts::handle_server_request;
use super::replies::handle_response;
use super::session_updates::parse_session_update;

/// Maps one JSON-RPC line from `agy acp` into Patchwork events, approvals, or follow-up writes.
#[must_use]
pub fn parse(v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
    let mut out = Vec::new();
    match (v.get("id"), v["method"].as_str()) {
        // Response to one of our JSON-RPC requests.
        (Some(id), None) => handle_response(id, v, spec, &mut out),
        // Server-initiated request (such as `session/request_permission`).
        (Some(_), Some(_)) => out.push(handle_server_request(v, spec)),
        // Server notification (`session/update`).
        (None, Some("session/update")) => {
            parse_session_update(&v["params"]["update"], spec, message_id, &mut out);
        }
        _ => {}
    }
    out
}

/// The task a shell command call runs, listed under the background when the reply ends while
/// it still runs: Antigravity answers its prompt once a long command moves to the background.
#[must_use]
pub fn background_task(tool: &ToolCall) -> Option<BackgroundTask> {
    if !matches!(
        tool.name.as_str(),
        "run_command" | "execute_command" | "shell" | "execute"
    ) {
        return None;
    }
    Some(BackgroundTask {
        tool_id: tool.id.clone(),
        task_id: None,
        run_id: None,
        kind: "command".into(),
        description: tool.summary.clone(),
        status: TaskStatus::Running,
        started_at: tool.started_at.unwrap_or_else(now_ms),
        summary: None,
        output: None,
        cut_off: false,
    })
}

/// Whether `line` shows Antigravity at work: text, a thought, a plan, or a call running.
///
/// It sends these too when it goes on working after answering its prompt. A call that only
/// ends, such as a background command, is not work.
#[must_use]
pub fn is_work(line: &Value) -> bool {
    let update = &line["params"]["update"];
    let ends_call = matches!(update["status"].as_str(), Some("completed" | "failed"));
    line.get("id").is_none()
        && line["method"] == "session/update"
        && match update["sessionUpdate"].as_str() {
            Some("agent_message_chunk" | "agent_thought_chunk" | "plan") => true,
            Some("tool_call" | "tool_call_update") => !ends_call,
            _ => false,
        }
}
