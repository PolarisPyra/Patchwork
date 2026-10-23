//! OpenCode CLI (`opencode run --format json`), versions 1.x and 2.x. Each turn is one run that
//! prints a JSON event per line and exits; nothing is written to its stdin.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{Value, json};

use crate::types::{
    AccessMode, AgentKind, AgentStatus, Attachment, AttachmentKind, ModelOption, Question,
    QuestionOption, TokenUsage,
};
use super::Parsed;
use super::ToolStart;
use super::TurnSpec;
use super::file_touched;
use super::find_binary;
use super::home_dir;
use super::one_line;
use super::opencode_bridge;
use super::probe;
use super::prompt;
use super::relative;
use super::summarize;
use super::text_delta;
use super::thinking;
use super::tool_finished;
use super::tool_started;

/// `opencode` on PATH or in the usual install folders, else where its installer puts it.
pub fn opencode_bin() -> Option<PathBuf> {
    find_binary("opencode").or_else(|| {
        home_dir()
            .map(|h| h.join(".opencode/bin/opencode"))
            .filter(|p| p.is_file())
    })
}

/// OpenCode's free models need no account, so an installed CLI can run a turn; other
/// providers are added with `opencode auth login`.
pub fn auth(status: &mut AgentStatus) {
    status.signed_in = opencode_bin().is_some();
    status.detail = if status.signed_in {
        "Using the OpenCode CLI — run `opencode auth login` to add providers".into()
    } else {
        "OpenCode CLI is not installed".into()
    };
}

/// The models OpenCode can use with the providers it is signed in to, as `provider/model`.
pub fn models() -> Vec<ModelOption> {
    let Some(listed) = probe(AgentKind::OpenCode, &["models"]).filter(|p| p.success) else {
        return Vec::new();
    };
    listed
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| line.contains('/') && !line.contains(char::is_whitespace))
        .map(|id| ModelOption {
            id: Some(id.into()),
            label: id.into(),
            default_model: None,
            is_legacy: false,
        })
        .collect()
}

/// The CLI's major version, read once.
fn major_version() -> u32 {
    static MAJOR: OnceLock<u32> = OnceLock::new();
    *MAJOR.get_or_init(|| {
        probe(AgentKind::OpenCode, &["--version"])
            .and_then(|p| {
                p.stdout
                    .split_whitespace()
                    .find_map(|w| w.trim_start_matches('v').split('.').next()?.parse().ok())
            })
            .unwrap_or(1)
    })
}

/// The tool kinds OpenCode 1.x checks before running; the read-only tools always run.
const GUARDED: [&str; 6] = [
    "bash",
    "webfetch",
    "websearch",
    "codesearch",
    "external_directory",
    "doom_loop",
];

/// Permission rules for the access mode, as `OPENCODE_CONFIG_CONTENT`. A run cannot answer a
/// prompt, so an "ask" rule refuses the call and the run goes on. None keeps OpenCode's own
/// rules: full access approves them with `--auto`, and auto lets OpenCode decide.
pub fn permission_config(spec: &TurnSpec) -> Option<String> {
    let edit = match spec.mode {
        AccessMode::FullAccess | AccessMode::Auto => return None,
        AccessMode::AutoAcceptEdits => "allow",
        AccessMode::Supervised => "ask",
    };
    let mut rules = serde_json::Map::new();
    rules.insert("edit".into(), json!(edit));
    rules.extend(GUARDED.map(|kind| (kind.to_string(), json!("ask"))));
    Some(json!({ "permission": rules }).to_string())
}

pub fn turn_args(spec: &TurnSpec) -> Vec<String> {
    // The bridge takes a 2.x turn from its stdin.
    if uses_serve() {
        return vec![opencode_bridge::BRIDGE_ARG.into()];
    }
    let mut args: Vec<String> = ["run", "--format", "json", "--thinking"].map(String::from).into();
    if spec.mode == AccessMode::FullAccess {
        args.push("--auto".into());
    }
    if spec.plan {
        args.extend(["--agent".into(), "plan".into()]);
    }
    if let Some(id) = spec.session_id {
        args.extend(["--session".into(), id.into()]);
    }
    if let Some(model) = spec.model {
        args.extend(["--model".into(), model.into()]);
    }
    if let Some(effort) = spec.effort {
        args.extend(["--variant".into(), effort.into()]);
    }
    // Images go with the message as files; every attachment is also named in the prompt.
    for image in spec.attachments.iter().filter(|a| a.kind == AttachmentKind::Image) {
        args.extend(["--file".into(), image.path.clone()]);
    }
    // After `--`, a prompt that starts with "-" is not read as a flag.
    args.extend(["--".into(), prompt(spec)]);
    args
}

/// Maps one line of `opencode run --format json`: a `step_start`, then the step's `reasoning`,
/// `text` and finished `tool_use` parts, then a `step_finish` with its token counts.
pub fn parse(v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
    if uses_serve() {
        return parse_serve(v, spec, message_id);
    }
    let part = &v["part"];
    match v["type"].as_str() {
        Some("step_start") => v["sessionID"]
            .as_str()
            .filter(|id| spec.session_id != Some(*id))
            .map(|id| Parsed::SessionId(id.into()))
            .into_iter()
            .collect(),
        Some("text") => text_of(part).map(|t| text_delta(message_id, t)).into_iter().collect(),
        Some("reasoning") => text_of(part).map(|t| thinking(message_id, t)).into_iter().collect(),
        Some("tool_use") => tool(part, spec.root, message_id),
        Some("step_finish") => step_usage(part).into_iter().collect(),
        Some("error") => {
            let error = &v["error"];
            // A stopped turn reports its own abort; that is not a failure.
            if error["name"] == "MessageAbortedError" {
                return Vec::new();
            }
            // 1.x puts the message under `data`, 2.x on the error itself.
            let message = error["data"]["message"]
                .as_str()
                .or_else(|| error["message"].as_str())
                .or_else(|| error["name"].as_str())
                .unwrap_or("OpenCode reported an error");
            vec![Parsed::Error(message.into())]
        }
        _ => Vec::new(),
    }
}

fn text_of(part: &Value) -> Option<&str> {
    part["text"].as_str().filter(|t| !t.trim().is_empty())
}

/// A tool part. OpenCode prints a tool once it has finished, so it starts and ends here.
fn tool(part: &Value, root: &str, message_id: &str) -> Vec<Parsed> {
    let state = &part["state"];
    // 1.x names the call with `callID`; 2.x has only the part's own `id`.
    let id = part["callID"].as_str().or_else(|| part["id"].as_str());
    let (Some(id), Some(name)) = (id, part["tool"].as_str()) else {
        return Vec::new();
    };
    let input = &state["input"];
    let path = input["filePath"].as_str().or_else(|| input["path"].as_str());
    let summary = match path {
        Some(path) => relative(root, path),
        None => summarize(input, root),
    };
    let mut out = vec![tool_started(
        message_id,
        ToolStart {
            id,
            name: tool_name(name),
            summary: one_line(&summary),
            subagent: None,
            parent: None,
        },
    )];
    let failed = state["status"] == "error";
    let output = if failed {
        state["error"].as_str()
    } else {
        state["output"].as_str()
    };
    out.push(tool_finished(message_id, id, failed, output.unwrap_or_default()));
    if !failed && matches!(name, "write" | "edit" | "patch") {
        out.extend(path.map(|p| file_touched(message_id, relative(root, p))));
    }
    out
}

/// OpenCode's tool names, shown the way the other agents name theirs.
fn tool_name(name: &str) -> &str {
    match name {
        "read" => "Read",
        "write" => "Write",
        "edit" | "patch" => "Edit",
        "bash" | "shell" => "Bash",
        "glob" => "Glob",
        "grep" => "Grep",
        "list" => "LS",
        "webfetch" => "WebFetch",
        "websearch" => "WebSearch",
        "todowrite" => "TodoWrite",
        "task" | "subagent" => "Task",
        "execute" => "Execute",
        other => other,
    }
}

/// The token counts of one model request, from its `step_finish` part.
fn step_usage(part: &Value) -> Option<Parsed> {
    let tokens = part.get("tokens")?;
    let count = |v: &Value| v.as_u64().unwrap_or(0);
    let input = count(&tokens["input"]);
    let cache_read = count(&tokens["cache"]["read"]);
    let cache_write = count(&tokens["cache"]["write"]);
    Some(Parsed::RequestUsage {
        request_id: part["id"].as_str()?.into(),
        usage: TokenUsage {
            input,
            cache_read,
            cache_write,
            output: count(&tokens["output"]) + count(&tokens["reasoning"]),
            context: input + cache_read + cache_write,
        },
        top_level: true,
    })
}

/// Whether turns run through `opencode serve`, which 2.x has; 1.x runs `opencode run`.
pub fn uses_serve() -> bool {
    major_version() >= 2
}

/// The bridge's first stdin line: everything the turn needs.
pub fn bridge_request(spec: &TurnSpec) -> String {
    let bin = opencode_bin().map(|b| b.display().to_string()).unwrap_or_default();
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
    })
    .to_string()
}

fn image_paths(attachments: &[Attachment]) -> Vec<String> {
    attachments
        .iter()
        .filter(|a| a.kind == AttachmentKind::Image)
        .map(|a| a.path.clone())
        .collect()
}

/// The session's permission rules for the access mode; the last matching rule wins. An empty
/// list keeps OpenCode's own rules, which Auto uses.
fn serve_permissions(spec: &TurnSpec) -> Value {
    let rule = |action: &str, effect: &str| json!({"action": action, "resource": "*", "effect": effect});
    let mut rules = match spec.mode {
        AccessMode::FullAccess => vec![rule("*", "allow")],
        AccessMode::Auto => Vec::new(),
        AccessMode::AutoAcceptEdits => {
            vec![rule("shell", "ask"), rule("edit", "allow"), rule("external_directory", "ask")]
        }
        AccessMode::Supervised => {
            vec![rule("shell", "ask"), rule("edit", "ask"), rule("external_directory", "ask")]
        }
    };
    // Plan mode only reads and plans.
    if spec.plan {
        rules.push(rule("edit", "deny"));
    }
    Value::Array(rules)
}

/// The bridge line that steers the running turn with another message.
pub fn steer_line(text: &str, attachments: &[Attachment]) -> String {
    json!({"type": "steer", "text": text, "files": image_paths(attachments)}).to_string()
}

/// The bridge line that answers a permission request or a question.
pub fn approval_reply(reply: &Value, allow: bool, answers: &HashMap<String, Vec<String>>) -> String {
    let request = &reply["requestID"];
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
fn parse_serve(v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
    let data = &v["data"];
    match v["type"].as_str() {
        Some("bridge.session") => data_str(v, "sessionID")
            .map(|id| Parsed::SessionId(id.into()))
            .into_iter()
            .collect(),
        Some("bridge.session_lost") => vec![Parsed::SessionLost],
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
            let Some(id) = data["id"].as_str() else { return Vec::new() };
            let failed = kind == "session.tool.failed";
            let output = if failed {
                data["error"]["message"].as_str().or_else(|| data["error"].as_str())
            } else {
                data["content"][0]["text"].as_str()
            };
            let mut out = vec![tool_finished(message_id, id, failed, output.unwrap_or_default())];
            let writes = matches!(data["name"].as_str(), Some("write" | "edit" | "patch"));
            if !failed && writes {
                out.extend(input_path(&data["input"]).map(|p| file_touched(message_id, relative(spec.root, p))));
            }
            out
        }
        Some("session.step.ended") => {
            let tokens = &data["tokens"];
            let count = |v: &Value| v.as_u64().unwrap_or(0);
            let input = count(&tokens["input"]);
            let cache_read = count(&tokens["cache"]["read"]);
            let cache_write = count(&tokens["cache"]["write"]);
            let Some(request_id) = v["id"].as_str() else { return Vec::new() };
            vec![Parsed::RequestUsage {
                request_id: request_id.into(),
                usage: TokenUsage {
                    input,
                    cache_read,
                    cache_write,
                    output: count(&tokens["output"]) + count(&tokens["reasoning"]),
                    context: input + cache_read + cache_write,
                },
                top_level: true,
            }]
        }
        Some("permission.asked" | "permission.v2.asked") => {
            let Some(request) = data["id"].as_str() else { return Vec::new() };
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
            }]
        }
        Some("question.asked" | "question.v2.asked") => {
            let Some(request) = data["id"].as_str() else { return Vec::new() };
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
                detail: questions.first().map(|q| q.question.clone()).unwrap_or_default(),
                reply: json!({"kind": "question", "requestID": request, "questions": ids}),
                questions,
            }]
        }
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
    input["path"].as_str().or_else(|| input["filePath"].as_str())
}

fn tool_summary(input: &Value, root: &str) -> String {
    match input_path(input) {
        Some(path) => relative(root, path),
        None => summarize(input, root),
    }
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
