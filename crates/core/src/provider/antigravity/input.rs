//! What Patchwork sends `agy acp`: arguments, JSON-RPC handshake, prompts, steering, and replies.

// Rust guideline compliant 2026-11-22

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use crate::chats::AccessMode;
use crate::provider::{Image, Parsed, TurnSpec, prompt};

use super::client::{client_info, strip_effort_suffix};
use super::questions;
use super::swarm;

/// Request id of `initialize`.
pub(super) const INIT_REQUEST: u64 = 1;

/// Request id of `session/new` or `session/resume`.
pub(super) const SESSION_REQUEST: u64 = 2;

/// Request id of `session/load`, for a session `session/resume` could not read.
pub(super) const LOAD_REQUEST: &str = "session-load";

/// Request id of `session/set_mode`.
pub(super) const MODE_REQUEST: u64 = 3;

/// Request id of `session/set_config_option` for the model.
pub(super) const MODEL_REQUEST: u64 = 4;

/// Request id of `session/set_config_option` for the thinking level.
pub(super) const EFFORT_REQUEST: u64 = 5;

/// First JSON-RPC request id for `session/prompt`; each prompt increments from here.
pub(super) const PROMPT_REQUEST: u64 = 6;

/// Monotonic id generator so every `session/prompt` in a warm ACP process is distinct.
static NEXT_PROMPT_ID: AtomicU64 = AtomicU64::new(PROMPT_REQUEST);

fn next_prompt_id() -> u64 {
    NEXT_PROMPT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Command-line arguments for an Antigravity ACP turn (`agy acp --stdio`).
#[must_use]
pub fn turn_args(spec: &TurnSpec) -> Vec<String> {
    let mut args: Vec<String> = ["acp", "--stdio"].map(String::from).into();
    if spec.mode == AccessMode::FullAccess {
        args.push("--dangerously-skip-permissions".into());
    }
    // Patchwork's permission hook (for deny rules even in FullAccess) and the user's hooks.
    if let Some(dir) = crate::provider::hooks::antigravity_dir(spec) {
        args.push(format!("--add-dir={}", dir.display()));
    }
    args
}

/// Environment for an Antigravity ACP turn: auto-compact window override when set.
#[must_use]
pub fn turn_env(spec: &TurnSpec) -> Vec<(&'static str, String)> {
    let mut env = Vec::new();
    if spec.compact_window > 0 {
        env.push((
            "AGY_CLI_AUTO_COMPACT_OVERRIDE",
            spec.compact_window.to_string(),
        ));
    }
    env
}

/// Handshake and session creation/resumption sent on stdin when the ACP process starts.
#[must_use]
pub fn turn_input(spec: &TurnSpec) -> Vec<String> {
    let init = json!({
        "jsonrpc": "2.0",
        "id": INIT_REQUEST,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientInfo": client_info(),
            // Keep fs/terminal false so `agy acp` uses its own workspace diff/shell tools,
            // and negotiate native Antigravity swarm notifications (`_antigravity/swarm`).
            "clientCapabilities": {
                "fs": {"readTextFile": false, "writeTextFile": false},
                "terminal": false,
                "_meta": swarm::capabilities_meta(),
            },
        },
    });
    let session = session_request(spec, spec.session_id);
    vec![init.to_string(), session.to_string()]
}

/// Builds `session/load` for `session_id`, which agy answers once it has replayed the session.
pub(super) fn load_request(spec: &TurnSpec, session_id: &str) -> Value {
    let mut request = session_request(spec, Some(session_id));
    request["id"] = json!(LOAD_REQUEST);
    request["method"] = json!("session/load");
    request
}

/// Builds `session/new` or `session/resume` with workspace root and optional system prompt append.
pub(super) fn session_request(spec: &TurnSpec, resume_id: Option<&str>) -> Value {
    let mut params = json!({"cwd": spec.root, "mcpServers": []});
    if let Some(text) = spec.instructions.filter(|t| !t.trim().is_empty()) {
        params["_meta"] = json!({"antigravity": {"systemPrompt": {"append": text}}});
    }
    match resume_id {
        Some(id) => {
            params["sessionId"] = json!(id);
            json!({
                "jsonrpc": "2.0",
                "id": SESSION_REQUEST,
                "method": "session/resume",
                "params": params,
            })
        }
        None => json!({
            "jsonrpc": "2.0",
            "id": SESSION_REQUEST,
            "method": "session/new",
            "params": params,
        }),
    }
}

/// What the chat wants agy's session to run with.
#[derive(Debug, Clone, Copy)]
pub struct Wanted<'a> {
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub mode: AccessMode,
}

/// The requests that bring agy's session from `config`, its `configOptions`, to `wanted`.
///
/// They are in the order to send them, each once agy answers the one before: agy answers
/// requests sent together out of order, and a later one can undo an earlier one.
#[must_use]
pub fn configure(session_id: &str, wanted: Wanted<'_>, config: &Value) -> Vec<String> {
    let option = |id: &str| {
        config
            .as_array()
            .into_iter()
            .flatten()
            .find(|opt| opt["id"].as_str() == Some(id))
    };
    let current = |id: &str| option(id).and_then(|opt| opt["currentValue"].as_str());
    let values = |id: &str| -> Vec<&str> {
        option(id)
            .and_then(|opt| opt["options"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|opt| opt["value"].as_str())
            .collect()
    };
    let set = |id: u64, config_id: &str, value: &str| {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "session/set_config_option",
            "params": {"sessionId": session_id, "configId": config_id, "value": value},
        })
        .to_string()
    };
    let mut out = Vec::new();

    if let Some(model) = wanted.model {
        let models = values("model");
        // A model named with its effort is offered without it when agy sets effort apart.
        let stripped = strip_effort_suffix(model);
        let resolved =
            if !models.contains(&model) && wanted.effort.is_some() && models.contains(&stripped) {
                stripped
            } else {
                model
            };
        if current("model") != Some(resolved) {
            out.push(set(MODEL_REQUEST, "model", resolved));
        }
    }

    if let Some(effort) = wanted.effort
        && current("thought_level") != Some(effort)
    {
        out.push(set(EFFORT_REQUEST, "thought_level", effort));
    }

    let modes = values("mode");
    let offered = |mode: &'static str| {
        if modes.contains(&mode) {
            mode
        } else {
            "default"
        }
    };
    let mode = match wanted.mode {
        AccessMode::FullAccess | AccessMode::Auto => offered("auto"),
        AccessMode::AutoAcceptEdits => offered("accept-edits"),
        AccessMode::Supervised => "default",
    };
    if current("mode") != Some(mode) {
        out.push(
            json!({
                "jsonrpc": "2.0",
                "id": MODE_REQUEST,
                "method": "session/set_mode",
                "params": {"sessionId": session_id, "modeId": mode},
            })
            .to_string(),
        );
    }
    out
}

/// Sets up the new session's model, effort and mode, then sends `session/prompt`.
pub(super) fn begin(spec: &TurnSpec, session_id: &str, session_result: &Value) -> Vec<Parsed> {
    let config = &session_result["configOptions"];
    let wanted = Wanted {
        model: spec.model,
        effort: spec.effort,
        mode: spec.mode,
    };
    let mut lines: Vec<Parsed> = configure(session_id, wanted, config)
        .into_iter()
        .map(Parsed::Send)
        .collect();
    let text = prompt(spec);
    let line = prompt_request(next_prompt_id(), session_id, &text, spec.images);
    // `agy` runs a slash command sent before it lists its commands as plain chat, so a
    // `/compact` that starts a resumed session waits for that list.
    lines.push(if text.trim_start().starts_with('/') {
        Parsed::SendAfterCommands(line)
    } else {
        Parsed::Send(line)
    });
    vec![
        Parsed::AgentConfig(config.clone()),
        Parsed::SendInOrder(lines),
    ]
}

/// Builds ACP content blocks for a prompt message and its base64-encoded image attachments.
fn prompt_blocks(text: &str, images: &[Image]) -> Vec<Value> {
    let mut blocks = Vec::new();
    if !text.is_empty() {
        blocks.push(json!({"type": "text", "text": text}));
    }
    for img in images {
        blocks.push(json!({
            "type": "image",
            "mimeType": img.media_type,
            "data": img.data,
        }));
    }
    blocks
}

/// Sends a new prompt into an idle, already-initialized ACP session with a fresh request id.
#[must_use]
pub fn prompt_input(session_id: &str, text: &str, images: &[Image]) -> String {
    prompt_request(next_prompt_id(), session_id, text, images)
}

fn prompt_request(id: u64, session_id: &str, text: &str, images: &[Image]) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": prompt_blocks(text, images),
        },
    })
    .to_string()
}

/// Cancels the active ACP prompt via the standard `session/cancel` notification.
#[must_use]
pub fn interrupt(session_id: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": {"sessionId": session_id},
    })
    .to_string()
}

/// Formats an ACP `session/request_permission` JSON-RPC response for an approval decision, or
/// for the user's answer when agy asked a question.
#[must_use]
pub fn approval_reply(
    reply: &Value,
    allow: bool,
    answers: &HashMap<String, Vec<String>>,
) -> String {
    let id = &reply["id"];
    let cancelled = reply["cancel"].as_bool() == Some(true);
    let outcome = if cancelled {
        json!({"outcome": "cancelled"})
    } else if reply["question"].is_string() {
        match questions::picked(reply, allow, answers) {
            Some(opt) => json!({"outcome": "selected", "optionId": opt}),
            None => json!({"outcome": "cancelled"}),
        }
    } else {
        let option_id = if allow {
            reply["allow_option"].as_str()
        } else {
            reply["deny_option"].as_str()
        };
        match option_id {
            Some(opt) => json!({"outcome": "selected", "optionId": opt}),
            None => json!({"outcome": "cancelled"}),
        }
    };
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"outcome": outcome},
    })
    .to_string()
}
