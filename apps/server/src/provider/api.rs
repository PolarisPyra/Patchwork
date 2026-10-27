//! API providers as an agent: a turn runs Patchwork's own agent (`api_bridge`) against the
//! provider of the task's model, and this maps the agent's `api.*` lines.

use std::collections::HashMap;

use serde_json::{Value, json};

use crate::types::{
    AgentEvent, AgentStatus, PlanStep, Question, QuestionOption, StepStatus, TokenUsage,
    ProviderSlashCommand,
};
use crate::permissions::Target;
use super::{Image, Parsed, ToolStart, TurnSpec};
use super::{api_bridge, api_config, api_tools, mcp, file_touched, prompt, text_delta, thinking, tool_finished, tool_started};

pub fn auth(status: &mut AgentStatus) {
    let providers = api_config::providers();
    let enabled = providers.iter().filter(|p| p.enabled).count();
    status.installed = enabled > 0;
    status.signed_in = status.installed;
    status.detail = match enabled {
        0 if providers.is_empty() => "Add a provider in Settings → API Providers".into(),
        0 => "Every API provider is turned off".into(),
        1 => "1 provider".into(),
        n => format!("{n} providers"),
    };
}

pub fn turn_args() -> Vec<String> {
    vec![api_bridge::BRIDGE_ARG.into()]
}

/// The first stdin line: the connection, the conversation's session, and the message. A turn
/// that cannot run, such as one whose provider is gone, gets a line the agent reports as is.
pub fn bridge_request(spec: &TurnSpec) -> String {
    let mut request = match api_config::connection(spec.model) {
        Ok(connection) => connection,
        Err(message) => return json!({"error": message}).to_string(),
    };
    let images: Vec<Value> = spec
        .images
        .iter()
        .map(|i| json!({"mediaType": i.media_type, "data": i.data}))
        .collect();
    let command = (spec.attachments.is_empty() && spec.text.trim() == "/compact").then_some("compact");
    let fields = json!({
        "root": spec.root,
        "sessionsDir": api_config::sessions_dir(),
        "sessionId": spec.session_id,
        "text": prompt(spec),
        "images": images,
        "mode": spec.mode,
        "plan": spec.plan,
        "effort": spec.effort,
        "instructions": spec.instructions,
        "command": command,
        "mcpTools": mcp::turn_tools(spec.plan),
    });
    if let (Some(request), Some(fields)) = (request.as_object_mut(), fields.as_object()) {
        request.extend(fields.clone());
    }
    request.to_string()
}

pub fn steer_line(text: &str, images: &[Image]) -> String {
    let images: Vec<Value> = images
        .iter()
        .map(|i| json!({"mediaType": i.media_type, "data": i.data}))
        .collect();
    json!({"type": "steer", "text": text, "images": images}).to_string()
}

/// The result of an MCP tool call, for the agent.
pub fn mcp_reply(id: &Value, result: Result<String, String>) -> String {
    let (output, error) = match result {
        Ok(text) => (text, false),
        Err(text) => (text, true),
    };
    json!({"type": "reply", "id": id, "output": output, "error": error}).to_string()
}

pub fn approval_reply(reply: &Value, allow: bool, answers: &HashMap<String, Vec<String>>) -> String {
    json!({"type": "reply", "id": reply["id"], "allow": allow, "answers": answers}).to_string()
}

/// An error a provider answered with, as one readable line.
pub fn error_text(status: u16, body: &Value, raw: &str) -> String {
    let error = &body["error"];
    let message = error["message"]
        .as_str()
        .or_else(|| error.as_str())
        .or_else(|| body["message"].as_str())
        .or_else(|| body["detail"].as_str())
        .map(String::from)
        .unwrap_or_else(|| raw.trim().chars().take(300).collect());
    let hint = match status {
        401 | 403 => " Check the API key in Settings → API Providers.",
        402 => " The account may be out of credit.",
        404 => " Check the base URL and the model id.",
        _ => "",
    };
    match status {
        0 => message,
        _ if message.is_empty() => format!("The provider answered {status}.{hint}"),
        _ => format!("{message} ({status}).{hint}"),
    }
}

pub fn parse(v: &Value, message_id: &str) -> Vec<Parsed> {
    let text = |key: &str| v[key].as_str().unwrap_or_default();
    match v["type"].as_str() {
        Some("api.session") => vec![Parsed::SessionId(text("id").into())],
        Some("api.session_lost") => vec![Parsed::SessionLost],
        Some("api.commands") => vec![Parsed::SlashCommands(
            v["commands"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| {
                    Some(ProviderSlashCommand {
                        name: c["name"].as_str()?.into(),
                        description: c["description"].as_str().map(String::from),
                    })
                })
                .collect(),
        )],
        Some("api.text") => vec![text_delta(message_id, text("delta"))],
        Some("api.reasoning") => vec![thinking(message_id, text("delta"))],
        Some("api.tool") => {
            let name = api_tools::display_name(text("name"));
            vec![tool_started(
                message_id,
                ToolStart { id: text("id"), name, summary: text("summary").into(), subagent: None, parent: None },
            )]
        }
        Some("api.tool_done") => {
            let mut out = vec![tool_finished(message_id, text("id"), v["error"] == true, text("output"))];
            if let Some(path) = v["path"].as_str() {
                out.push(file_touched(message_id, path.into()));
            }
            out
        }
        Some("api.todos") => {
            let steps = v["steps"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|s| PlanStep {
                    text: s["text"].as_str().unwrap_or_default().into(),
                    status: match s["status"].as_str() {
                        Some("done" | "completed") => StepStatus::Done,
                        Some("running" | "in_progress") => StepStatus::Running,
                        _ => StepStatus::Pending,
                    },
                })
                .collect();
            vec![Parsed::Event(AgentEvent::PlanUpdated { message_id: message_id.into(), steps })]
        }
        Some("api.plan") => vec![Parsed::Event(AgentEvent::PlanProposed {
            message_id: message_id.into(),
            text: text("text").into(),
        })],
        Some("api.approval") => vec![Parsed::Approval {
            title: text("title").into(),
            detail: super::one_line(text("detail")),
            reply: json!({"id": v["id"]}),
            questions: Vec::new(),
            target: serde_json::from_value(v["kind"].clone()).ok().map(|kind| {
                let resources = v["resources"].as_array().into_iter().flatten().filter_map(Value::as_str);
                Target::new(kind, resources)
            }),
        }],
        Some("api.question") => {
            let questions: Vec<Question> = v["questions"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|q| Question {
                    id: q["id"].as_str().unwrap_or_default().into(),
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
                    multi_select: q["multiSelect"] == true,
                })
                .collect();
            vec![Parsed::Approval {
                title: "Question".into(),
                detail: questions.first().map(|q| q.question.clone()).unwrap_or_default(),
                reply: json!({"id": v["id"]}),
                questions,
                target: None,
            }]
        }
        Some("api.usage") => {
            let count = |key: &str| v[key].as_u64().unwrap_or(0);
            vec![Parsed::RequestUsage {
                request_id: text("id").into(),
                usage: TokenUsage {
                    input: count("input"),
                    cache_read: count("cacheRead"),
                    cache_write: 0,
                    output: count("output"),
                    context: count("context"),
                },
                top_level: true,
            }]
        }
        Some("api.retry") => vec![Parsed::Retrying {
            reason: text("reason").into(),
            attempt: v["attempt"].as_u64().unwrap_or(1) as u32,
            max_attempts: v["max"].as_u64().unwrap_or(3) as u32,
        }],
        Some("api.mcp_call") => vec![Parsed::McpCall {
            id: v["id"].clone(),
            server: text("server").into(),
            tool: text("tool").into(),
            arguments: v["arguments"].clone(),
        }],
        Some("api.steered") => vec![Parsed::Steered],
        Some("api.error") => vec![Parsed::Error(text("message").into())],
        _ => Vec::new(),
    }
}
