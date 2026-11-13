//! What a turn asks of OpenCode's server: the request, its permissions, steering and approvals.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use serde_json::{Value, json};
use crate::provider::ProviderSlashCommand;
use crate::threads::{AccessMode, Attachment, AttachmentKind};
use crate::permissions::PermissionKind;
use crate::provider::TurnSpec;
use crate::provider::prompt;
use crate::provider::opencode::{major_version, opencode_bin};

/// Whether turns run through `opencode serve`, which 2.x has; 1.x runs `opencode run`.
#[must_use]
pub fn uses_serve() -> bool {
    major_version() >= 2
}

/// The bridge's first stdin line: everything the turn needs.
#[must_use]
pub fn bridge_request(spec: &TurnSpec) -> String {
    let bin = opencode_bin()
        .map(|b| b.display().to_string())
        .unwrap_or_default();
    json!({
        "bin": bin,
        "root": spec.root,
        "sessionId": spec.session_id,
        "model": spec.model,
        "effort": spec.effort,
        "agent": if spec.plan { "plan" } else { "build" },
        "permissions": serve_permissions(spec),
        "text": prompt(spec),
        "files": image_paths(spec.attachments),
        "command": command_of(spec),
        "compactWindow": spec.compact_window,
        "systemPrompt": spec.instructions,
    })
    .to_string()
}

/// A message that is only `/name` and its arguments, as `{name, text}`. The bridge runs it as
/// one of OpenCode's commands when the session has one by that name, else sends it as a prompt.
pub(super) fn command_of(spec: &TurnSpec) -> Option<Value> {
    if !spec.attachments.is_empty() {
        return None;
    }
    let rest = spec.text.trim().strip_prefix('/')?;
    let (name, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    (!name.is_empty() && !name.contains('/'))
        .then(|| json!({ "name": name, "text": text.trim_start() }))
}

/// OpenCode's commands for the slash menu.
pub(super) fn slash_commands(list: &Value) -> Vec<ProviderSlashCommand> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let name = c["name"].as_str()?.trim();
            let description = c["description"]
                .as_str()
                .map(str::trim)
                .filter(|d| !d.is_empty());
            (!name.is_empty()).then(|| ProviderSlashCommand {
                name: name.into(),
                description: description.map(String::from),
            })
        })
        .collect()
}

pub(super) fn image_paths(attachments: &[Attachment]) -> Vec<String> {
    attachments
        .iter()
        .filter(|a| a.kind == AttachmentKind::Image)
        .map(|a| a.path.clone())
        .collect()
}

/// The session's permission rules for the access mode; the last matching rule wins.
///
/// Every action a rule can answer asks Patchwork first, in every mode, and Patchwork reads its
/// rules when the request arrives: a rule added, changed, or removed while the turn runs then
/// decides the calls after it, and answers one already held. Without a rule the access mode
/// decides, as before, so only calls the rules hold reach the user.
pub(super) fn serve_permissions(spec: &TurnSpec) -> Value {
    let rule =
        |action: &str, effect: &str| json!({"action": action, "resource": "*", "effect": effect});
    let mut rules = match spec.mode {
        // Full access and Auto approve everything the ask rules below do not name. Auto's own
        // judgement goes with them: the rules now decide, as they do in every other mode.
        AccessMode::FullAccess | AccessMode::Auto => vec![rule("*", "allow")],
        // Supervised and accept-edits asked about these actions before; the ask rules below say
        // which, so their own entries would only repeat them.
        AccessMode::Supervised | AccessMode::AutoAcceptEdits => Vec::new(),
    };
    // The actions `permission_target` can name, which are the calls a rule can answer. MCP tools
    // ask under their id, `<server>_<tool>`, which every id with an underscore matches; that
    // covers OpenCode's own two as well, and Patchwork leaves those to the mode when no rule
    // names them.
    for kind in [
        PermissionKind::Edit,
        PermissionKind::Command,
        PermissionKind::Folder,
        PermissionKind::Web,
    ] {
        if let Some(action) = action(kind) {
            rules.push(rule(action, "ask"));
        }
    }
    rules.push(rule("*_*", "ask"));
    // A tool caught in a loop has no rule and no request for Patchwork to answer, so it stays
    // OpenCode's own call, as before.
    rules.push(rule("doom_loop", "allow"));
    // Plan mode only reads and plans.
    if spec.plan {
        rules.push(rule("edit", "deny"));
    }
    Value::Array(rules)
}

/// The action OpenCode asks permission for, for a kind of request.
pub(in crate::provider::opencode) fn action(kind: PermissionKind) -> Option<&'static str> {
    match kind {
        PermissionKind::Edit => Some("edit"),
        PermissionKind::Command => Some("shell"),
        PermissionKind::Folder => Some("external_directory"),
        PermissionKind::Web => Some("webfetch"),
        // OpenCode asks for no screen access; only Patchwork's own agent uses the screen.
        PermissionKind::Mcp | PermissionKind::Screen => None,
    }
}

/// Whether the access mode asks the user about a kind of request; OpenCode's own rules, which
/// auto keeps, ask only about other folders.
#[must_use]
pub fn mode_asks(mode: AccessMode, kind: PermissionKind) -> bool {
    match mode {
        AccessMode::FullAccess => false,
        AccessMode::Auto => kind == PermissionKind::Folder,
        AccessMode::AutoAcceptEdits => {
            matches!(kind, PermissionKind::Command | PermissionKind::Folder)
        }
        AccessMode::Supervised => kind != PermissionKind::Web && kind != PermissionKind::Mcp,
    }
}

/// The bridge line that steers the running turn with another message.
#[must_use]
pub fn steer_line(text: &str, attachments: &[Attachment]) -> String {
    json!({"type": "steer", "text": text, "files": image_paths(attachments)}).to_string()
}

/// The bridge line that answers a permission request or a question.
#[must_use]
pub fn approval_reply(
    reply: &Value,
    allow: bool,
    answers: &HashMap<String, Vec<String>>,
) -> String {
    let request = &reply["requestID"];
    if reply["kind"] == "form" {
        if !allow {
            return json!({"type": "form_cancel", "requestID": request}).to_string();
        }
        // A multiselect field takes a list; any other takes its answer as text.
        let mut answer = serde_json::Map::new();
        for field in reply["fields"].as_array().into_iter().flatten() {
            let Some(key) = field["key"].as_str() else {
                continue;
            };
            let Some(picks) = answers.get(key).filter(|p| !p.is_empty()) else {
                continue;
            };
            let value = if field["type"] == "multiselect" {
                json!(picks)
            } else {
                json!(picks.join(", "))
            };
            answer.insert(key.into(), value);
        }
        return json!({"type": "form", "requestID": request, "answer": answer}).to_string();
    }
    if reply["kind"] == "question" {
        if !allow {
            return json!({"type": "question_reject", "requestID": request}).to_string();
        }
        // Answers go back in the order of the questions.
        let ordered: Vec<Vec<String>> = reply["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| id.as_str())
            .map(|id| answers.get(id).cloned().unwrap_or_default())
            .collect();
        return json!({"type": "question", "requestID": request, "answers": ordered}).to_string();
    }
    let decision = if allow { "once" } else { "reject" };
    let mut line = json!({"type": "permission", "requestID": request, "decision": decision});
    // Why a hook refused the call, which OpenCode hands to the model with the refusal.
    if let Some(message) = reply["message"].as_str() {
        line["message"] = message.into();
    }
    line.to_string()
}
