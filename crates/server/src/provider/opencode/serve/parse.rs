//! Reading what the server sends back into the events of a turn.

// Rust guideline compliant 2026-10-09

use super::request::slash_commands;
use serde_json::{Value, json};
use crate::threads::{Question, QuestionOption, TokenUsage};
use crate::permissions::Target;
use crate::permissions::PermissionKind;
use crate::provider::Parsed;
use crate::provider::ToolStart;
use crate::provider::TurnSpec;
use crate::provider::file_touched;
use crate::provider::one_line;
use crate::provider::relative;
use crate::provider::summarize;
use crate::provider::text_delta;
use crate::provider::thinking;
use crate::provider::tool_finished;
use crate::provider::tool_started;
use crate::provider::opencode::{todo_plan, tool_name};

/// Maps one line from the bridge: the session's `opencode serve` events, and its own
/// `bridge.*` lines.
pub(in crate::provider::opencode) fn parse_serve(
    v: &Value,
    spec: &TurnSpec,
    message_id: &str,
) -> Vec<Parsed> {
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
        Some("bridge.error" | "session.execution.failed") => vec![Parsed::Error(message_of(v))],
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
        Some("session.compaction.ended") => vec![Parsed::Compacted(
            data["text"]
                .as_str()
                .filter(|t| !t.is_empty())
                .map(String::from),
        )],
        Some("session.retry.scheduled") => vec![Parsed::Retrying {
            reason: crate::provider::one_line(&message_of(v)),
            attempt: u32::try_from(data["attempt"].as_u64().unwrap_or(0)).unwrap_or(u32::MAX),
            max_attempts: 0,
        }],
        _ => Vec::new(),
    }
}

pub(super) fn data_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v[key].as_str().or_else(|| v["data"][key].as_str())
}

/// The message of a bridge line or a failed run.
pub(super) fn message_of(v: &Value) -> String {
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
pub(super) fn input_path(input: &Value) -> Option<&str> {
    input["path"]
        .as_str()
        .or_else(|| input["filePath"].as_str())
}

pub(super) fn tool_summary(input: &Value, root: &str) -> String {
    match input_path(input) {
        Some(path) => relative(root, path),
        None => summarize(input, root),
    }
}

/// What a permission request asks to do, for the permission rules.
pub(super) fn permission_target(action: &str, resources: &[&str]) -> Option<Target> {
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
pub(super) fn permission_title(action: &str) -> &str {
    match action {
        "edit" => "Edit files",
        "shell" | "bash" => "Run a command",
        "external_directory" => "Work outside the folder",
        "webfetch" | "websearch" => "Use the web",
        other => other,
    }
}
