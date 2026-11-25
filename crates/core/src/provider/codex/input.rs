//! What Patchwork sends `codex app-server`: its arguments, and the JSON-RPC requests that start, steer and stop a turn.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use std::collections::HashMap;

use serde_json::{Value, json};

use crate::chats::AccessMode;
use crate::chats::Attachment;
use crate::chats::AttachmentKind;
use crate::provider::Parsed;
use crate::provider::prompt;
use crate::provider::TurnSpec;

use super::{configured_model, models};

/// `codex app-server` speaks JSON-RPC on stdio; `turn_input` starts the conversation.
///
/// Codex leaves out its `update_plan` tool unless asked, and that tool is what fills the plan above
/// the chat box. Its `request_user_input` tool, the question card, works only in plan mode unless
/// enabled.
pub fn turn_args() -> Vec<String> {
    [
        "app-server",
        "-c",
        "tools.update_plan.enabled=true",
        "-c",
        "features.default_mode_request_user_input=true",
    ]
    .map(String::from)
    .into()
}

/// A set compact window makes Codex compact once its context reaches it.
#[must_use]
pub fn compact_args(spec: &TurnSpec) -> Vec<String> {
    if spec.compact_window == 0 {
        return Vec::new();
    }
    vec![
        "-c".into(),
        format!("model_auto_compact_token_limit={}", spec.compact_window),
    ]
}

/// The custom agents as the `agents` table of a chat's config, so Codex's `spawn_agent` can start
/// them by name. A role's instructions and model live in a file of their own, which the role
/// points to. None when there are none.
///
/// They go in `thread/start` and `thread/resume`, which reach Codex on its input, rather than as
/// `-c` overrides: those go on the command line, which Windows refuses past 32,767 characters,
/// and every custom agent would add to it whether or not the chat uses one. An agent the chat is
/// set to or summons reaches Codex through the message, not this list.
fn agents_config(spec: &TurnSpec) -> Option<Value> {
    // Listing the models runs the CLI, which a turn with no custom agent has no use for.
    if spec.agents.is_empty() {
        return None;
    }
    let models = models();
    let mut roles = serde_json::Map::new();
    for agent in spec.agents {
        // JSON strings are TOML strings too.
        let mut role = format!("developer_instructions = {}\n", json!(agent.prompt.trim()));
        // Codex can't run another CLI's model, so the role takes the chat's.
        if let Some(model) = agent.model.as_deref().and_then(|m| {
            crate::extensions::agents::offered(m, models.iter().filter_map(|o| o.id.as_deref()))
        }) {
            let _ = writeln!(role, "model = {}", json!(model));
        }
        match crate::extensions::agents::launch_file(
            "codex",
            spec.root,
            &format!("{}.toml", agent.name),
            &role,
        ) {
            Ok(file) => {
                roles.insert(
                    agent.name.clone(),
                    json!({"description": agent.about(), "config_file": file}),
                );
            }
            Err(e) => log::warn!(
                agent:% = agent.name, error:% = e;
                "failed to hand Codex the {{agent}} agent: {{error}}"
            ),
        }
    }
    (!roles.is_empty()).then(|| json!({"agents": roles}))
}

/// Request id of `chat/start` / `chat/resume`; its response carries the chat id.
pub(super) const CHAT_REQUEST: u64 = 2;

/// Parameters of `chat/start`, or of `chat/resume` when `resume` names the chat.
pub(super) fn chat_params(spec: &TurnSpec, resume: Option<&str>) -> Value {
    let mut chat = json!({"cwd": spec.root});
    if let Some(m) = spec.model {
        chat["model"] = json!(m);
    }
    if let Some(text) = spec.instructions {
        chat["developerInstructions"] = json!(text);
    }
    if let Some(config) = agents_config(spec) {
        chat["config"] = config;
    }
    if let Some(id) = resume {
        chat["threadId"] = json!(id);
        chat["excludeTurns"] = json!(true);
    }
    chat
}

/// Handshake, then start or resume the chat. `turn/start` follows once the chat id is known.
#[must_use]
pub fn turn_input(spec: &TurnSpec) -> Vec<String> {
    let chat = chat_params(spec, spec.session_id);
    let method = match spec.session_id {
        Some(_) => "thread/resume",
        None => "thread/start",
    };
    [
        json!({"id": 1, "method": "initialize", "params": {
            "clientInfo": {"name": "patchwork", "title": "Patchwork", "version": crate::VERSION},
            // turn/start collaborationMode is part of Codex's experimental app-server API.
            "capabilities": {"experimentalApi": true}
        }}),
        json!({"method": "initialized"}),
        // Patchwork's skills folder joins Codex's own skill folders.
        json!({"id": SKILLS_REQUEST, "method": "skills/extraRoots/set", "params": {
            "extraRoots": spec.skills_dir.into_iter().collect::<Vec<_>>(),
        }}),
        json!({"id": CHAT_REQUEST, "method": method, "params": chat}),
    ]
    .map(|v| v.to_string())
    .into()
}

/// Request id of `skills/extraRoots/set`. Its response, even an error from a Codex without the
/// method, does not end the turn.
pub(super) const SKILLS_REQUEST: u64 = 12;

/// Request id of `turn/start`.
pub(super) const TURN_REQUEST: u64 = CHAT_REQUEST + 1;

/// Request id of `chat/goal/clear`; its response needs nothing.
pub(super) const GOAL_CLEAR_REQUEST: u64 = 10;

/// What the conversation's first request does once the chat id is known: start the turn.
/// A resumed chat first drops any goal Codex kept from before Patchwork ran goals itself, so
/// it does not pursue one of its own beside Patchwork's.
pub(super) fn begin(spec: &TurnSpec, chat_id: &str, resolved_model: Option<&str>) -> Vec<Parsed> {
    let clear = spec.session_id.map(|_| {
        Parsed::Send(
            json!({"id": GOAL_CLEAR_REQUEST, "method": "thread/goal/clear", "params": {"threadId": chat_id}})
                .to_string(),
        )
    });
    // Codex compacts in a turn of its own, which it starts and reports like any other.
    let request = if spec.text.trim() == "/compact" && spec.attachments.is_empty() {
        json!({"id": TURN_REQUEST, "method": "thread/compact/start", "params": {"threadId": chat_id}})
            .to_string()
    } else {
        turn_start(spec, chat_id, &prompt(spec), resolved_model)
    };
    clear.into_iter().chain([Parsed::Send(request)]).collect()
}

/// The approval policy, reviewer, and sandbox for each access mode.
fn turn_start(spec: &TurnSpec, chat_id: &str, text: &str, resolved_model: Option<&str>) -> String {
    let (policy, reviewer, sandbox) = match spec.mode {
        AccessMode::Supervised => ("untrusted", "user", "readOnly"),
        AccessMode::AutoAcceptEdits => ("on-request", "user", "workspaceWrite"),
        AccessMode::Auto => ("on-request", "auto_review", "workspaceWrite"),
        AccessMode::FullAccess => ("never", "user", "dangerFullAccess"),
    };
    let mut params = json!({
        "threadId": chat_id,
        "cwd": spec.root,
        "input": if text.is_empty() { vec![] } else { input_items(text, spec.attachments, spec.skills_dir) },
        "approvalPolicy": policy,
        "approvalsReviewer": reviewer,
        "sandboxPolicy": {"type": sandbox},
    });
    if let Some(m) = spec.model {
        params["model"] = json!(m);
    }
    if let Some(e) = spec.effort {
        params["effort"] = json!(e);
    }
    // Each turn runs in Codex's default collaboration mode, so a session left in another mode
    // leaves it. The mode names its model; without a model to name, it is not sent.
    if let Some(model) = spec
        .model
        .map(String::from)
        .or_else(|| resolved_model.map(String::from))
        .or_else(configured_model)
    {
        params["collaborationMode"] = json!({"mode": "default", "settings": {
            "model": model,
            "reasoning_effort": spec.effort,
            "developer_instructions": null,
        }});
    }
    json!({"id": TURN_REQUEST, "method": "turn/start", "params": params}).to_string()
}

fn input_items(text: &str, attachments: &[Attachment], skills_dir: Option<&str>) -> Vec<Value> {
    let mut input = vec![json!({"type": "text", "text": text})];
    // A message that invokes a skill names it as "$name", which Codex takes with the skill.
    if let Some(dir) = skills_dir {
        let skills = crate::extensions::skills::list(std::path::Path::new(dir));
        if let Some(skill) = crate::extensions::skills::mentioned(text)
            .find_map(|name| skills.iter().find(|s| s.name == name))
        {
            input.push(json!({"type": "skill", "name": skill.name, "path": skill.path}));
        }
    }
    input.extend(
        attachments
            .iter()
            .filter(|a| a.kind == AttachmentKind::Image)
            .map(|a| json!({"type": "localImage", "path": a.path})),
    );
    input
}

/// Request id of `turn/steer`; its response is not a reason to end the turn.
pub(super) const STEER_REQUEST: u64 = 4;

/// `turn/steer` adds input to the turn that is running now, identified by its native id.
#[must_use]
pub fn steer_input(chat_id: &str, turn_id: &str, text: &str, attachments: &[Attachment]) -> String {
    json!({"id": STEER_REQUEST, "method": "turn/steer", "params": {
        "threadId": chat_id,
        "expectedTurnId": turn_id,
        "input": input_items(text, attachments, None),
    }})
    .to_string()
}

pub(super) const INTERRUPT_REQUEST: u64 = 13;

/// `turn/interrupt` stops the turn running now; Codex ends it with `turn/completed` and the
/// status `interrupted`.
#[must_use]
pub fn interrupt(chat_id: &str, turn_id: &str) -> String {
    json!({"id": INTERRUPT_REQUEST, "method": "turn/interrupt", "params": {
        "threadId": chat_id,
        "turnId": turn_id,
    }})
    .to_string()
}

/// A question's reply carries the answers by question id; declining it answers none.
#[must_use]
pub fn approval_reply(
    reply: &Value,
    allow: bool,
    answers: &HashMap<String, Vec<String>>,
) -> String {
    if reply["question"] == true {
        let answers: serde_json::Map<_, _> = answers
            .iter()
            .filter(|_| allow)
            .map(|(id, picks)| (id.clone(), json!({"answers": picks})))
            .collect();
        return json!({"id": reply["id"], "result": {"answers": answers}}).to_string();
    }
    let decision = if allow { "accept" } else { "decline" };
    json!({"id": reply["id"], "result": {"decision": decision}}).to_string()
}
