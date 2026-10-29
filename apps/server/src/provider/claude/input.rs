//! What Patchwork sends Claude Code: its arguments, and the lines it writes to its stdin.

use std::collections::HashMap;

use serde_json::Value;

use crate::threads::AccessMode;
use crate::provider::Image;
use crate::provider::TurnSpec;

/// The prompt is sent on stdin (`user_message`), so approvals can be answered there too.
pub fn turn_args(spec: &TurnSpec) -> Vec<String> {
    let mode = match spec.mode {
        _ if spec.plan => "plan",
        AccessMode::Supervised => "manual",
        AccessMode::AutoAcceptEdits => "acceptEdits",
        AccessMode::Auto => "auto",
        AccessMode::FullAccess => "bypassPermissions",
    };
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        // A subagent's own text and reasoning, for its thread; without this only its calls arrive.
        "--forward-subagent-text",
        // Newer models send no reasoning text unless asked, so a long think streamed nothing
        // and the reply looked hung.
        "--thinking-display",
        "summarized",
        "--permission-mode",
        mode,
    ]
    .map(String::from)
    .into();
    // Permission checks arrive as `can_use_tool` control requests on stdout. Full access
    // takes them too, since AskUserQuestion asks the user that way in every mode.
    args.extend(["--permission-prompt-tool".to_string(), "stdio".to_string()]);
    if let Some(id) = spec.session_id {
        args.extend(["--resume".to_string(), id.to_string()]);
    }
    if let Some(m) = spec.model {
        args.extend(["--model".to_string(), m.to_string()]);
    }
    if let Some(e) = spec.effort {
        args.extend(["--effort".to_string(), e.to_string()]);
    }
    args
}

/// The stream-json input line carrying the prompt and any attached images.
pub fn user_message(text: &str, images: &[Image]) -> String {
    user_line(text, images, None)
}

/// A message for a turn that is already running. `priority: "now"` asks Claude to
/// take it before its next step rather than after the turn.
pub fn steer_message(text: &str, images: &[Image]) -> String {
    user_line(text, images, Some("now"))
}

pub(super) fn user_line(text: &str, images: &[Image], priority: Option<&str>) -> String {
    let mut content = vec![serde_json::json!({"type": "text", "text": text})];
    content.extend(images.iter().map(|i| {
        serde_json::json!({
            "type": "image",
            "source": {"type": "base64", "media_type": i.media_type, "data": i.data}
        })
    }));
    let mut message = serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": content},
        "parent_tool_use_id": null,
        "session_id": "",
    });
    if let Some(priority) = priority {
        message["priority"] = priority.into();
    }
    message.to_string()
}

/// The answer to ExitPlanMode: the plan was taken, so Claude stops here.
pub(super) const PLAN_CAPTURED: &str = "The client captured your proposed plan. Stop here and wait for the \
user's feedback or implementation request in a later turn.";

/// Asks Claude to move the call `tool_id` (a subagent working in the foreground) to the
/// background, where it keeps working; Claude answers with a control response.
pub fn background_task(tool_id: &str, request_id: &str) -> String {
    serde_json::json!({
        "type": "control_request",
        "request_id": request_id,
        "request": {"subtype": "background_tasks", "tool_use_id": tool_id},
    })
    .to_string()
}

/// Asks Claude to stop one of its subagents.
pub fn stop_task(task_id: &str) -> String {
    serde_json::json!({
        "type": "control_request",
        "request_id": crate::system::new_id(),
        "request": {"subtype": "stop_task", "task_id": task_id},
    })
    .to_string()
}

/// Asks Claude to stop its reply. It ends the reply with a `result` whose `terminal_reason` is
/// `aborted_streaming` or `aborted_tools`, and stays up for the next message.
pub fn interrupt() -> String {
    serde_json::json!({
        "type": "control_request",
        "request_id": crate::system::new_id(),
        "request": {"subtype": "interrupt"},
    })
    .to_string()
}

/// Answers go back in the tool's input, each under its question's text, with several picks
/// joined by commas.
pub fn approval_reply(
    reply: &Value,
    allow: bool,
    answers: &HashMap<String, Vec<String>>,
) -> String {
    let decision = if allow {
        let mut input = reply["input"].clone();
        if !answers.is_empty() {
            input["answers"] = answers
                .iter()
                .map(|(question, picks)| (question.clone(), picks.join(", ").into()))
                .collect::<serde_json::Map<_, _>>()
                .into();
        }
        serde_json::json!({"behavior": "allow", "updatedInput": input})
    } else {
        serde_json::json!({"behavior": "deny", "message": "The user denied this action."})
    };
    serde_json::json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": reply["request_id"],
            "response": decision,
        },
    })
    .to_string()
}
