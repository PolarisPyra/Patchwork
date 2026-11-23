//! What Patchwork sends `agy acp`: arguments, JSON-RPC handshake, prompts, steering, and replies.

// Rust guideline compliant 2026-11-22

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use crate::chats::AccessMode;
use crate::provider::{Image, Parsed, TurnSpec, prompt};

use super::client::{client_info, strip_effort_suffix};
use super::swarm;

/// Request id of `initialize`.
pub(super) const INIT_REQUEST: u64 = 1;

/// Request id of `session/new` or `session/resume`.
pub(super) const SESSION_REQUEST: u64 = 2;

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

/// Builds `session/new` or `session/resume` with workspace root and optional system prompt append.
pub(super) fn session_request(spec: &TurnSpec, resume_id: Option<&str>) -> Value {
    let mut params = json!({"cwd": spec.root});
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

/// Configures mode, model, and effort once the session is ready, then sends `session/prompt`.
pub(super) fn begin(spec: &TurnSpec, session_id: &str, session_result: &Value) -> Vec<Parsed> {
    let mut out = Vec::new();

    // Pick the ACP session mode matching Patchwork's AccessMode.
    let available_modes: Vec<&str> = session_result["modes"]["availableModes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str())
        .collect();
    let current_mode = session_result["modes"]["currentModeId"].as_str();
    let wanted_mode = match spec.mode {
        AccessMode::FullAccess | AccessMode::Auto => {
            if available_modes.contains(&"auto") {
                "auto"
            } else {
                "default"
            }
        }
        AccessMode::AutoAcceptEdits => {
            if available_modes.contains(&"accept-edits") {
                "accept-edits"
            } else {
                "default"
            }
        }
        AccessMode::Supervised => "default",
    };
    if current_mode != Some(wanted_mode) {
        out.push(Parsed::Send(
            json!({
                "jsonrpc": "2.0",
                "id": MODE_REQUEST,
                "method": "session/set_mode",
                "params": {"sessionId": session_id, "modeId": wanted_mode},
            })
            .to_string(),
        ));
    }

    // Apply model selection only when it differs from the session's active value.
    let config_options = session_result["configOptions"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let model_config = config_options
        .iter()
        .find(|opt| opt["id"].as_str() == Some("model"));
    let current_model = model_config.and_then(|opt| opt["currentValue"].as_str());
    let model_options: Vec<&str> = model_config
        .and_then(|opt| opt["options"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|opt| opt["value"].as_str())
        .collect();
    if let Some(m) = spec.model {
        let resolved = if model_options.contains(&m) {
            m
        } else {
            let stripped = strip_effort_suffix(m);
            if spec.effort.is_some() && model_options.contains(&stripped) {
                stripped
            } else {
                m
            }
        };
        if current_model != Some(resolved) {
            out.push(Parsed::Send(
                json!({
                    "jsonrpc": "2.0",
                    "id": MODEL_REQUEST,
                    "method": "session/set_config_option",
                    "params": {
                        "sessionId": session_id,
                        "configId": "model",
                        "value": resolved,
                    },
                })
                .to_string(),
            ));
        }
    }

    // Apply thinking effort only when it differs from the session's active value.
    let current_effort = config_options
        .iter()
        .find(|opt| opt["id"].as_str() == Some("thought_level"))
        .and_then(|opt| opt["currentValue"].as_str());
    if let Some(effort) = spec.effort
        && current_effort != Some(effort)
    {
        out.push(Parsed::Send(
            json!({
                "jsonrpc": "2.0",
                "id": EFFORT_REQUEST,
                "method": "session/set_config_option",
                "params": {
                    "sessionId": session_id,
                    "configId": "thought_level",
                    "value": effort,
                },
            })
            .to_string(),
        ));
    }

    let text = prompt(spec);
    let line = prompt_request(next_prompt_id(), session_id, &text, spec.images);
    // `agy` runs a slash command sent before it lists its commands as plain chat, so a
    // `/compact` that starts a resumed session waits for that list.
    out.push(if text.trim_start().starts_with('/') {
        Parsed::SendAfterCommands(line)
    } else {
        Parsed::Send(line)
    });
    out
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

/// Formats an ACP `session/request_permission` JSON-RPC response for an approval decision.
#[must_use]
pub fn approval_reply(
    reply: &Value,
    allow: bool,
    _answers: &HashMap<String, Vec<String>>,
) -> String {
    let id = &reply["id"];
    let cancelled = reply["cancel"].as_bool() == Some(true);
    let outcome = if cancelled {
        json!({"outcome": "cancelled"})
    } else {
        let option_id = if allow {
            reply["allow_option"].as_str().or(Some("allow"))
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
