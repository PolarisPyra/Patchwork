//! What Patchwork sends an ACP agent (`agy acp` and the listed ones): arguments, JSON-RPC
//! handshake, prompts, steering, and replies.

// Rust guideline compliant 2026-11-23

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use crate::chats::{AccessMode, Attachment, AttachmentKind};
use crate::provider::{AgentKind, Image, Parsed, TurnSpec, acp, prompt};

use super::models::strip_effort_suffix;
use super::probe::client_info;
use super::questions;

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

/// Largest attached text file sent along with the prompt; a bigger one is left for agy to read.
const MAX_TEXT_ATTACHMENT_BYTES: u64 = 1024 * 1024;

fn next_prompt_id() -> u64 {
    NEXT_PROMPT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Command-line arguments for an ACP turn: `agy acp --stdio`, or the listed agent's own.
#[must_use]
pub fn turn_args(spec: &TurnSpec) -> Vec<String> {
    if let Some(acp) = acp::spec(spec.agent) {
        return acp.args.iter().map(|a| (*a).to_string()).collect();
    }
    // Full Access runs as agy's "auto" mode with Patchwork answering what agy still asks, so the
    // mode can change in the session without a new process.
    let mut args: Vec<String> = ["acp", "--stdio"].map(String::from).into();
    // Patchwork's permission hook (for deny rules even in FullAccess) and the user's hooks.
    if let Some(dir) = crate::provider::hooks::antigravity_dir(spec) {
        args.push(format!("--add-dir={}", dir.display()));
    }
    args
}

/// Environment for an ACP turn: the listed agent's own, or agy's auto-compact window override
/// when set.
#[must_use]
pub fn turn_env(spec: &TurnSpec) -> Vec<(&'static str, String)> {
    if spec.agent.is_listed_acp() {
        return acp::env(spec.agent);
    }
    let mut env = Vec::new();
    if spec.compact_window > 0 {
        env.push((
            "AGY_CLI_AUTO_COMPACT_OVERRIDE",
            spec.compact_window.to_string(),
        ));
    }
    env
}

/// Handshake and session creation/resumption sent on stdin when the ACP process starts. A listed
/// agent is sent only the handshake; its answer says whether a session can be loaded.
#[must_use]
pub fn turn_input(spec: &TurnSpec) -> Vec<String> {
    let init = json!({
        "jsonrpc": "2.0",
        "id": INIT_REQUEST,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientInfo": client_info(),
            // Keep fs/terminal false so `agy acp` uses its own workspace diff/shell tools.
            "clientCapabilities": {
                "fs": {"readTextFile": false, "writeTextFile": false},
                "terminal": false,
            },
        },
    });
    if spec.agent.is_listed_acp() {
        return vec![init.to_string()];
    }
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

/// Builds `session/new` or `session/resume` with the workspace root and Patchwork's tools.
pub(super) fn session_request(spec: &TurnSpec, resume_id: Option<&str>) -> Value {
    // Patchwork's own tools run on this machine, which a remote agy cannot start.
    let servers = if spec.host.is_some() {
        json!([])
    } else {
        crate::provider::mcp::overrides::acp_servers(spec.chat_id)
    };
    let mut params = json!({"cwd": spec.root, "mcpServers": servers});
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

/// What the chat wants the agent's session to run with.
#[derive(Debug, Clone, Copy)]
pub struct Wanted<'a> {
    pub agent: AgentKind,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub mode: AccessMode,
}

/// The requests that bring the agent's session from `config`, its `configOptions`, to `wanted`.
///
/// They are in the order to send them, each once the agent answers the one before: agy answers
/// requests sent together out of order, and a later one can undo an earlier one.
#[must_use]
pub fn configure(session_id: &str, wanted: Wanted<'_>, config: &Value) -> Vec<String> {
    let option = |category: &str| {
        config
            .as_array()
            .into_iter()
            .flatten()
            .find(|opt| opt["id"] == category || opt["category"] == category)
    };
    let current = |category: &str| option(category).and_then(|opt| opt["currentValue"].as_str());
    let values = |category: &str| -> Vec<&str> {
        option(category)
            .and_then(|opt| opt["options"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|opt| opt["value"].as_str())
            .collect()
    };
    let request = |id: u64, method: &str, params: Value| {
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
    };
    // An option from the config list is set by its own id; one made from an older `models` or
    // `modes` list by that list's own method.
    let set = |id: u64, category: &str, value: &str, legacy: (&str, &str)| {
        let opt = option(category);
        if opt.is_some_and(|o| o[acp::LEGACY] == true) {
            let (method, key) = legacy;
            request(id, method, json!({"sessionId": session_id, key: value}))
        } else {
            let config_id = opt.and_then(|o| o["id"].as_str()).unwrap_or(category);
            request(
                id,
                "session/set_config_option",
                json!({"sessionId": session_id, "configId": config_id, "value": value}),
            )
        }
    };
    let agy = wanted.agent == AgentKind::Antigravity;
    let mut out = Vec::new();

    if let Some(model) = wanted.model {
        let models = values("model");
        // A model named with its effort is offered without it when agy sets effort apart.
        let stripped = strip_effort_suffix(model);
        let resolved = if agy
            && !models.contains(&model)
            && wanted.effort.is_some()
            && models.contains(&stripped)
        {
            stripped
        } else {
            model
        };
        // A listed agent is sent only a model its session offers.
        if current("model") != Some(resolved) && (agy || models.contains(&resolved)) {
            out.push(set(
                MODEL_REQUEST,
                "model",
                resolved,
                ("session/set_model", "modelId"),
            ));
        }
    }

    if let Some(effort) = wanted.effort
        && current("thought_level") != Some(effort)
        && (agy || values("thought_level").contains(&effort))
    {
        out.push(set(EFFORT_REQUEST, "thought_level", effort, ("", "")));
    }

    let modes = values("mode");
    let mode = if agy {
        let offered = |mode: &'static str| {
            if modes.contains(&mode) {
                mode
            } else {
                "default"
            }
        };
        Some(match wanted.mode {
            AccessMode::FullAccess | AccessMode::Auto => offered("auto"),
            AccessMode::AutoAcceptEdits => offered("accept-edits"),
            AccessMode::Supervised => "default",
        })
    } else {
        acp::spec(wanted.agent)
            .and_then(|s| acp::mode_for(s, wanted.mode))
            .filter(|mode| modes.contains(mode))
    };
    if let Some(mode) = mode
        && current("mode") != Some(mode)
    {
        out.push(if agy {
            request(
                MODE_REQUEST,
                "session/set_mode",
                json!({"sessionId": session_id, "modeId": mode}),
            )
        } else {
            set(MODE_REQUEST, "mode", mode, ("session/set_mode", "modeId"))
        });
    }
    out
}

/// Sets up the new session's model, effort and mode, then sends `session/prompt`.
pub(super) fn begin(spec: &TurnSpec, session_id: &str, session_result: &Value) -> Vec<Parsed> {
    let config = &if spec.agent.is_listed_acp() {
        let config = acp::config_options(session_result);
        acp::remember_models(spec.agent, &config);
        config
    } else {
        session_result["configOptions"].clone()
    };
    let wanted = Wanted {
        agent: spec.agent,
        model: spec.model,
        effort: spec.effort,
        mode: spec.mode,
    };
    let mut lines: Vec<Parsed> = configure(session_id, wanted, config)
        .into_iter()
        .map(Parsed::Send)
        .collect();
    let mut text = acp::own_command(spec.agent, prompt(spec));
    // A listed agent takes no system prompt at launch, so a new session's first message has it.
    if spec.agent.is_listed_acp()
        && spec.session_id.is_none()
        && let Some(instructions) = spec.instructions.filter(|i| !i.trim().is_empty())
    {
        text = format!("{instructions}\n\n{text}");
    }
    let line = prompt_request(
        next_prompt_id(),
        session_id,
        &prompt_blocks(&text, spec.attachments, spec.images),
    );
    // `agy` runs a slash command sent before it lists its commands as plain chat, so a
    // `/compact` that starts a resumed session waits for that list.
    lines.push(
        if spec.agent == AgentKind::Antigravity && text.trim_start().starts_with('/') {
            Parsed::SendAfterCommands(line)
        } else {
            Parsed::Send(line)
        },
    );
    vec![
        Parsed::AgentConfig(config.clone()),
        Parsed::SendInOrder(lines),
    ]
}

/// Builds ACP content blocks for a prompt: its text, the images, each attached text file's
/// contents, and a link to each attached PDF. The text still lists every attachment's path, so a
/// file left out here (too big, not text, or unreadable) is one agy can still open itself.
fn prompt_blocks(text: &str, attachments: &[Attachment], images: &[Image]) -> Vec<Value> {
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
    blocks.extend(
        attachments
            .iter()
            .filter(|a| a.kind == AttachmentKind::File)
            .filter_map(file_block),
    );
    blocks
}

/// The content block for an attached file: a link for a PDF, which agy reads itself, or the
/// text of a UTF-8 text file. None for any other file.
fn file_block(attachment: &Attachment) -> Option<Value> {
    let uri = url::Url::from_file_path(&attachment.path).ok()?.to_string();
    let size = std::fs::metadata(&attachment.path).ok()?.len();
    let pdf = std::path::Path::new(&attachment.path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"));
    if pdf {
        return Some(json!({
            "type": "resource_link",
            "uri": uri,
            "name": attachment.name,
            "mimeType": "application/pdf",
            "size": size,
        }));
    }
    if size > MAX_TEXT_ATTACHMENT_BYTES {
        return None;
    }
    let text = String::from_utf8(std::fs::read(&attachment.path).ok()?).ok()?;
    if text.contains('\0') {
        return None;
    }
    Some(json!({
        "type": "resource",
        "resource": {"uri": uri, "mimeType": "text/plain", "text": text},
    }))
}

/// Sends a new prompt into an idle, already-initialized ACP session with a fresh request id.
#[must_use]
pub fn prompt_input(
    session_id: &str,
    text: &str,
    attachments: &[Attachment],
    images: &[Image],
) -> String {
    prompt_request(
        next_prompt_id(),
        session_id,
        &prompt_blocks(text, attachments, images),
    )
}

fn prompt_request(id: u64, session_id: &str, blocks: &[Value]) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": blocks,
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
