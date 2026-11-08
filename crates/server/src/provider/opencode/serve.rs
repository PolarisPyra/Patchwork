//! OpenCode 2.x: a turn runs through Patchwork's bridge to a private `opencode serve`, and its events, permissions and answers.

use std::collections::HashMap;
use serde_json::{Value, json};
use crate::provider::ProviderSlashCommand;
use crate::threads::{AccessMode, Attachment, AttachmentKind, Question, QuestionOption, TokenUsage};
use crate::permissions::Target;
use crate::permissions::PermissionKind;
use crate::provider::Parsed;
use crate::provider::ToolStart;
use crate::provider::TurnSpec;
use crate::provider::file_touched;
use crate::provider::one_line;
use crate::provider::prompt;
use crate::provider::relative;
use crate::provider::summarize;
use crate::provider::text_delta;
use crate::provider::thinking;
use crate::provider::tool_finished;
use crate::provider::tool_started;
use super::{major_version, opencode_bin, todo_plan, tool_name};

/// Whether turns run through `opencode serve`, which 2.x has; 1.x runs `opencode run`.
pub fn uses_serve() -> bool {
    major_version() >= 2
}

/// The bridge's first stdin line: everything the turn needs.
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
fn command_of(spec: &TurnSpec) -> Option<Value> {
    if !spec.attachments.is_empty() {
        return None;
    }
    let rest = spec.text.trim().strip_prefix('/')?;
    let (name, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    (!name.is_empty() && !name.contains('/'))
        .then(|| json!({ "name": name, "text": text.trim_start() }))
}

/// OpenCode's commands for the slash menu.
fn slash_commands(list: &Value) -> Vec<ProviderSlashCommand> {
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

fn image_paths(attachments: &[Attachment]) -> Vec<String> {
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
fn serve_permissions(spec: &TurnSpec) -> Value {
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
pub(super) fn action(kind: PermissionKind) -> Option<&'static str> {
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
pub fn steer_line(text: &str, attachments: &[Attachment]) -> String {
    json!({"type": "steer", "text": text, "files": image_paths(attachments)}).to_string()
}

/// The bridge line that answers a permission request or a question.
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
    json!({"type": "permission", "requestID": request, "decision": decision}).to_string()
}

/// Maps one line from the bridge: the session's `opencode serve` events, and its own
/// `bridge.*` lines.
pub(super) fn parse_serve(v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
    let data = &v["data"];
    match v["type"].as_str() {
        Some("bridge.session") => data_str(v, "sessionID")
            .map(|id| Parsed::SessionId(id.into()))
            .into_iter()
            .collect(),
        Some("bridge.session_lost") => vec![Parsed::SessionLost],
        Some("bridge.commands") => vec![Parsed::SlashCommands(slash_commands(&v["commands"]))],
        Some("bridge.steered") => vec![Parsed::Steered],
        Some("bridge.steer_rejected") => vec![Parsed::SteerRejected(message_of(v))],
        Some("bridge.error") => vec![Parsed::Error(message_of(v))],
        Some("session.text.delta") => data["delta"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map(|t| text_delta(message_id, t))
            .into_iter()
            .collect(),
        Some("session.reasoning.delta") => data["delta"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map(|t| thinking(message_id, t))
            .into_iter()
            .collect(),
        Some("session.tool.called") => {
            let (Some(id), Some(name)) = (data["id"].as_str(), data["name"].as_str()) else {
                return Vec::new();
            };
            vec![tool_started(
                message_id,
                ToolStart {
                    id,
                    name: tool_name(name),
                    summary: one_line(&tool_summary(&data["input"], spec.root)),
                    subagent: None,
                    parent: None,
                },
            )]
        }
        Some(kind @ ("session.tool.success" | "session.tool.failed")) => {
            let Some(id) = data["id"].as_str() else {
                return Vec::new();
            };
            let failed = kind == "session.tool.failed";
            let output = if failed {
                data["error"]["message"]
                    .as_str()
                    .or_else(|| data["error"].as_str())
            } else {
                data["content"][0]["text"].as_str()
            };
            let mut out = vec![tool_finished(
                message_id,
                id,
                failed,
                output.unwrap_or_default(),
            )];
            let writes = matches!(data["name"].as_str(), Some("write" | "edit" | "patch"));
            if !failed && writes {
                out.extend(
                    input_path(&data["input"])
                        .map(|p| file_touched(message_id, relative(spec.root, p))),
                );
            }
            if !failed {
                out.extend(
                    data["name"]
                        .as_str()
                        .and_then(|name| todo_plan(name, &data["input"], message_id)),
                );
            }
            out
        }
        Some("session.step.ended") => {
            let tokens = &data["tokens"];
            let count = |v: &Value| v.as_u64().unwrap_or(0);
            let input = count(&tokens["input"]);
            let cache_read = count(&tokens["cache"]["read"]);
            let cache_write = count(&tokens["cache"]["write"]);
            let Some(request_id) = v["id"].as_str() else {
                return Vec::new();
            };
            vec![Parsed::RequestUsage {
                request_id: request_id.into(),
                usage: TokenUsage {
                    input,
                    cache_read,
                    cache_write,
                    output: count(&tokens["output"]) + count(&tokens["reasoning"]),
                    context: input + cache_read + cache_write,
                    ..Default::default()
                },
                top_level: true,
            }]
        }
        Some("permission.asked" | "permission.v2.asked") => {
            let Some(request) = data["id"].as_str() else {
                return Vec::new();
            };
            let action = data["action"].as_str().unwrap_or("act");
            let resources: Vec<&str> = data["resources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            vec![Parsed::Approval {
                title: permission_title(action).into(),
                detail: one_line(&resources.join(", ")),
                reply: json!({"kind": "permission", "requestID": request}),
                questions: Vec::new(),
                target: permission_target(action, &resources),
            }]
        }
        Some("question.asked" | "question.v2.asked") => {
            let Some(request) = data["id"].as_str() else {
                return Vec::new();
            };
            let questions: Vec<Question> = data["questions"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(i, q)| Question {
                    id: format!("q{i}"),
                    header: q["header"].as_str().unwrap_or_default().into(),
                    question: q["question"].as_str().unwrap_or_default().into(),
                    options: q["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|o| QuestionOption {
                            label: o["label"].as_str().unwrap_or_default().into(),
                            description: o["description"].as_str().unwrap_or_default().into(),
                        })
                        .collect(),
                    multi_select: q["multiple"] == true,
                })
                .collect();
            let ids: Vec<&str> = questions.iter().map(|q| q.id.as_str()).collect();
            vec![Parsed::Approval {
                title: "Question".into(),
                detail: questions
                    .first()
                    .map(|q| q.question.clone())
                    .unwrap_or_default(),
                reply: json!({"kind": "question", "requestID": request, "questions": ids}),
                questions,
                target: None,
            }]
        }
        // OpenCode 2.1 asks its question tool's questions as a form, one field per question.
        Some("form.created") => {
            let form = &data["form"];
            let Some(request) = form["id"].as_str() else {
                return Vec::new();
            };
            let fields: Vec<&Value> = form["fields"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|f| f["key"].is_string() && f["hidden"] != true)
                .collect();
            let questions: Vec<Question> = fields
                .iter()
                .map(|f| Question {
                    id: f["key"].as_str().unwrap_or_default().into(),
                    header: f["title"].as_str().unwrap_or_default().into(),
                    question: f["description"]
                        .as_str()
                        .or_else(|| f["title"].as_str())
                        .unwrap_or_default()
                        .into(),
                    options: f["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|o| QuestionOption {
                            label: o["label"].as_str().unwrap_or_default().into(),
                            description: o["description"].as_str().unwrap_or_default().into(),
                        })
                        .collect(),
                    multi_select: f["type"] == "multiselect",
                })
                .collect();
            let shapes: Vec<Value> = fields
                .iter()
                .map(|f| json!({"key": f["key"], "type": f["type"]}))
                .collect();
            vec![Parsed::Approval {
                title: "Question".into(),
                detail: questions
                    .first()
                    .map(|q| q.question.clone())
                    .unwrap_or_default(),
                reply: json!({"kind": "form", "requestID": request, "fields": shapes}),
                questions,
                target: None,
            }]
        }
        Some("session.compaction.started") => vec![Parsed::Compacting],
        Some("session.execution.failed") => vec![Parsed::Error(message_of(v))],
        _ => Vec::new(),
    }
}

fn data_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v[key].as_str().or_else(|| v["data"][key].as_str())
}

/// The message of a bridge line or a failed run.
fn message_of(v: &Value) -> String {
    let error = &v["data"]["error"];
    v["message"]
        .as_str()
        .or_else(|| error["message"].as_str())
        .or_else(|| error["data"]["message"].as_str())
        .or_else(|| error.as_str())
        .unwrap_or("OpenCode reported an error")
        .into()
}

/// 2.x names a file `path`; 1.x `filePath`.
fn input_path(input: &Value) -> Option<&str> {
    input["path"]
        .as_str()
        .or_else(|| input["filePath"].as_str())
}

fn tool_summary(input: &Value, root: &str) -> String {
    match input_path(input) {
        Some(path) => relative(root, path),
        None => summarize(input, root),
    }
}

/// What a permission request asks to do, for the permission rules.
fn permission_target(action: &str, resources: &[&str]) -> Option<Target> {
    let kind = match action {
        "edit" => PermissionKind::Edit,
        "shell" | "bash" => PermissionKind::Command,
        "external_directory" => PermissionKind::Folder,
        "webfetch" => PermissionKind::Web,
        // An MCP tool asks under its id, `<server>_<tool>`.
        _ if action.contains('_') && action != "doom_loop" => {
            return Some(Target::new(PermissionKind::Mcp, [action]));
        }
        _ => return None,
    };
    Some(Target::new(kind, resources.iter().copied()))
}

/// What a permission request asks to do, as an approval title.
fn permission_title(action: &str) -> &str {
    match action {
        "edit" => "Edit files",
        "shell" | "bash" => "Run a command",
        "external_directory" => "Work outside the folder",
        "webfetch" | "websearch" => "Use the web",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::serve_permissions;
    use crate::provider::TurnSpec;
    use crate::threads::AccessMode;

    fn spec(mode: AccessMode, plan: bool) -> TurnSpec<'static> {
        TurnSpec {
            root: "/w",
            thread_id: "thread",
            session_id: None,
            model: None,
            vision_model: None,
            text: "go",
            mode,
            plan,
            effort: None,
            compact_window: 0,
            attachments: &[],
            images: &[],
            instructions: None,
            skills_dir: None,
            guarded: &[],
            agents: &[],
            hooks: &[],
        }
    }

    /// The rule an action ends up with, which is its last matching one.
    fn effect_rules(rules: &serde_json::Value) -> Vec<(String, String)> {
        rules
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                Some((
                    r["action"].as_str()?.to_string(),
                    r["effect"].as_str()?.to_string(),
                ))
            })
            .collect()
    }

    fn effect_of(rules: &serde_json::Value, action: &str) -> Option<String> {
        effect_rules(rules)
            .into_iter()
            .rfind(|(a, _)| a == action)
            .map(|(_, effect)| effect)
    }

    /// Every action a rule can answer asks Patchwork, in every mode, so a rule saved mid-turn
    /// decides the next call. OpenCode's own loop guard keeps its own call.
    #[test]
    fn every_action_a_rule_can_answer_asks_patchwork() {
        for mode in [
            AccessMode::Supervised,
            AccessMode::AutoAcceptEdits,
            AccessMode::Auto,
            AccessMode::FullAccess,
        ] {
            let rules = serve_permissions(&spec(mode, false));
            for action in ["shell", "edit", "external_directory", "webfetch"] {
                assert_eq!(
                    effect_of(&rules, action).as_deref(),
                    Some("ask"),
                    "{mode:?} asks Patchwork about {action}: {rules}"
                );
            }
            // MCP tools ask under their id, `<server>_<tool>`.
            assert_eq!(effect_of(&rules, "*_*").as_deref(), Some("ask"), "{mode:?}");
            assert_eq!(
                effect_of(&rules, "doom_loop").as_deref(),
                Some("allow"),
                "{mode:?}"
            );
        }
    }

    #[test]
    fn plan_mode_still_refuses_edits() {
        let rules = serve_permissions(&spec(AccessMode::Auto, true));
        assert_eq!(
            effect_of(&rules, "edit").as_deref(),
            Some("deny"),
            "{rules}"
        );
    }
}

#[cfg(test)]
mod dump_rules {
    #[test]
    fn print_rules() {
        for (mode, plan) in [
            (crate::threads::AccessMode::Supervised, false),
            (crate::threads::AccessMode::AutoAcceptEdits, false),
            (crate::threads::AccessMode::Auto, false),
            (crate::threads::AccessMode::FullAccess, false),
            (crate::threads::AccessMode::Supervised, true),
        ] {
            let spec = crate::provider::TurnSpec {
                root: "/w",
                thread_id: "thread",
                session_id: None,
                model: None,
                vision_model: None,
                text: "go",
                mode,
                plan,
                effort: None,
                compact_window: 0,
                attachments: &[],
                images: &[],
                instructions: None,
                skills_dir: None,
                guarded: &[],
                agents: &[],
                hooks: &[],
            };
            println!(
                "RULES {mode:?} plan={plan} {}",
                super::serve_permissions(&spec)
            );
        }
    }
}
