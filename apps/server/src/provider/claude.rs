//! Claude Code (`claude -p` with stream-json input and output).

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::permissions::Target;
use crate::types::AccessMode;
use crate::types::AgentEvent;
use crate::types::AgentKind;
use crate::types::AgentStatus;
use crate::types::BackgroundTask;
use crate::types::ClaudeModel;
use crate::types::GoalStatus;
use crate::types::ModelOption;
use crate::types::now_ms;
use crate::types::PlanStep;
use crate::types::ProviderSlashCommand;
use crate::types::PermissionKind;
use crate::types::Question;
use crate::types::QuestionOption;
use crate::types::StepStatus;
use crate::types::Refusal;
use crate::types::SubAgent;
use crate::types::TaskStatus;
use crate::types::TokenUsage;
use super::file_touched;
use super::GoalUpdate;
use super::home_dir;
use super::Image;
use super::claude_cli;
use super::model_manifest;
use super::without_attachments;
use super::one_line;
use super::Parsed;
use super::TaskEvent;
use super::probe;
use super::relative;
use super::summarize;
use super::text_delta;
use super::thinking;
use super::tool_finished;
use super::tool_started;
use super::ToolStart;
use super::TurnSpec;

/// `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".claude")))
}

/// Claude Code's settings file: `.claude.json` in `CLAUDE_CONFIG_DIR`, or in the home folder.
pub fn settings_file() -> PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir).join(".claude.json"),
        None => home_dir().unwrap_or_default().join(".claude.json"),
    }
}

pub fn auth(status: &mut AgentStatus) {
    let json = probe(AgentKind::Claude, &["auth", "status"])
        .and_then(|p| serde_json::from_str::<Value>(p.stdout.trim()).ok());
    let logged_in = json
        .as_ref()
        .and_then(|v| v["loggedIn"].as_bool())
        .unwrap_or(false);
    if logged_in {
        let method = json
            .as_ref()
            .and_then(|v| v["authMethod"].as_str())
            .unwrap_or("claude.ai");
        status.signed_in = true;
        status.detail = format!("Signed in with {method}");
    } else {
        status.detail = "Not signed in — run `claude` to log in".into();
    }
}

/// Read only model settings, without launching an inference request.
pub fn configured_model() -> Option<String> {
    if let Ok(model) = std::env::var("ANTHROPIC_MODEL")
        && !model.trim().is_empty()
    {
        return Some(model);
    }
    let settings: Value =
        serde_json::from_str(&std::fs::read_to_string(home()?.join("settings.json")).ok()?).ok()?;
    Some(settings.get("model")?.as_str()?.to_owned())
}

/// Current Claude models, from the fetched model manifest, each with the thinking levels the
/// cached catalog says it takes. The manifest names the models; the catalog is the CLI's own
/// record of what each one can do, so a model that cannot think gets no level.
pub fn models() -> Vec<ModelOption> {
    let catalog = claude_cli::models();
    let mut models = model_manifest::models("claude");
    for model in &mut models {
        if let Some(id) = model.id.as_deref() {
            model.efforts = efforts_in(&catalog, id);
        }
    }
    models
}

/// The levels `id` takes, from the catalog. A catalog id matches exactly, or a dated snapshot of
/// it: `claude-haiku-4-5` matches `claude-haiku-4-5-20251001`. Only a date counts as a snapshot,
/// so `claude-opus-5` never takes the levels of `claude-opus-5-5`.
fn efforts_in(catalog: &[ClaudeModel], id: &str) -> Option<Vec<String>> {
    // A pinned context window, as in `claude-fable-5-1[1m]`, is not part of the catalog's id.
    let id = id.split('[').next().unwrap_or(id);
    let exact = catalog.iter().find(|model| model.id == id);
    let model = exact.or_else(|| {
        catalog.iter().find(|model| {
            model
                .id
                .strip_prefix(id)
                .and_then(|rest| rest.strip_prefix('-'))
                .is_some_and(is_snapshot_date)
        })
    })?;
    Some(model.efforts.clone())
}

/// A catalog id's `YYYYMMDD` snapshot date, which marks a dated copy of a model.
fn is_snapshot_date(text: &str) -> bool {
    text.len() >= 6 && text.chars().all(|c| c.is_ascii_digit())
}

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

fn user_line(text: &str, images: &[Image], priority: Option<&str>) -> String {
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
const PLAN_CAPTURED: &str = "The client captured your proposed plan. Stop here and wait for the \
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
        "request_id": crate::types::new_id(),
        "request": {"subtype": "stop_task", "task_id": task_id},
    })
    .to_string()
}

/// Asks Claude to stop its reply. It ends the reply with a `result` whose `terminal_reason` is
/// `aborted_streaming` or `aborted_tools`, and stays up for the next message.
pub fn interrupt() -> String {
    serde_json::json!({
        "type": "control_request",
        "request_id": crate::types::new_id(),
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

/// What a subagent did in one of its frames (whole `assistant` and `user` messages that name the
/// call that started it): its text, reasoning, and calls, for its own thread.
pub fn subagent_events(v: &Value, root: &str, message_id: &str) -> Vec<AgentEvent> {
    let blocks = v["message"]["content"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let mut parsed = Vec::new();
    match v["type"].as_str() {
        Some("assistant") => {
            for block in blocks {
                match block["type"].as_str() {
                    Some("text") => {
                        let text = block["text"].as_str().unwrap_or_default();
                        if !text.trim().is_empty() {
                            parsed.push(text_delta(message_id, text));
                        }
                    }
                    Some("thinking") => {
                        let text = block["thinking"].as_str().unwrap_or_default();
                        if !text.trim().is_empty() {
                            let now = now_ms();
                            parsed.push(Parsed::Event(AgentEvent::Thinking {
                                message_id: message_id.into(),
                                text: text.into(),
                                started_at: Some(now),
                                ended_at: Some(now),
                            }));
                        }
                    }
                    Some("tool_use") => claude_tool_use(block, root, message_id, None, &mut parsed),
                    _ => {}
                }
            }
        }
        Some("user") => {
            parsed.extend(
                blocks
                    .iter()
                    .filter(|b| b["type"] == "tool_result")
                    .map(|block| tool_result(block, message_id)),
            );
        }
        _ => {}
    }
    parsed
        .into_iter()
        .filter_map(|p| match p {
            Parsed::Event(event) => Some(event),
            _ => None,
        })
        .collect()
}

/// A `tool_result` block: the call it answers is finished, with its output.
fn tool_result(block: &Value, message_id: &str) -> Parsed {
    let id = block["tool_use_id"].as_str().unwrap_or_default();
    let is_error = block["is_error"].as_bool().unwrap_or(false);
    // Content is a string, or text blocks.
    let content = &block["content"];
    let output = content.as_str().map_or_else(
        || block_text(content.as_array().map_or(&[][..], Vec::as_slice)),
        String::from,
    );
    tool_finished(message_id, id, is_error, &output)
}

/// A `can_use_tool` request becomes an approval; other control requests are refused.
fn claude_control_request(v: &Value, root: &str) -> Parsed {
    let request = &v["request"];
    if request["subtype"] != "can_use_tool" {
        return Parsed::Send(
            serde_json::json!({
                "type": "control_response",
                "response": {
                    "subtype": "error",
                    "request_id": v["request_id"],
                    "error": "Not supported by Patchwork",
                },
            })
            .to_string(),
        );
    }
    let tool = request["tool_name"].as_str().unwrap_or("a tool");
    let reply = serde_json::json!({"request_id": v["request_id"], "input": request["input"]});
    if tool == "AskUserQuestion" {
        let questions = claude_questions(&request["input"]);
        return Parsed::Approval {
            title: "Question".into(),
            detail: questions
                .first()
                .map(|q| q.question.clone())
                .unwrap_or_default(),
            reply,
            questions,
            target: None,
        };
    }
    let title = match tool {
        "Bash" => "Run command".to_string(),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => "Edit file".to_string(),
        _ => format!("Use {}", request["display_name"].as_str().unwrap_or(tool)),
    };
    let mut detail = summarize(&request["input"], root);
    if detail.is_empty() {
        detail = one_line(request["description"].as_str().unwrap_or_default());
    }
    Parsed::Approval {
        title,
        detail,
        reply,
        questions: vec![],
        target: claude_target(tool, &request["input"], root),
    }
}

/// What a tool call asks to do, for the permission rules.
fn claude_target(tool: &str, input: &Value, root: &str) -> Option<Target> {
    let text = |key: &str| input[key].as_str().unwrap_or_default();
    let path = input["file_path"]
        .as_str()
        .or(input["notebook_path"].as_str());
    match tool {
        "Bash" => Some(Target::new(PermissionKind::Command, [text("command")])),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            Some(Target::new(PermissionKind::Edit, path))
        }
        // Reading outside the folder: the file's folder, or the folder searched.
        "Read" => {
            let path = std::path::Path::new(path?);
            let folder = path.parent().unwrap_or(path).to_string_lossy().into_owned();
            Some(Target::new(PermissionKind::Folder, [folder]))
        }
        "Glob" | "Grep" | "LS" => Some(Target::new(
            PermissionKind::Folder,
            [input["path"].as_str().unwrap_or(root)],
        )),
        "WebFetch" => Some(Target::new(PermissionKind::Web, [text("url")])),
        _ => {
            let (server, tool) = tool.strip_prefix("mcp__")?.split_once("__")?;
            Some(Target::new(
                PermissionKind::Mcp,
                [format!("{server}/{tool}")],
            ))
        }
    }
}

/// AskUserQuestion's questions. Claude takes each answer back under the question's text.
fn claude_questions(input: &Value) -> Vec<Question> {
    let questions = input["questions"].as_array().map_or(&[][..], Vec::as_slice);
    questions
        .iter()
        .filter_map(|q| {
            let question = q["question"].as_str()?.to_string();
            let options = q["options"].as_array().map_or(&[][..], Vec::as_slice);
            Some(Question {
                id: question.clone(),
                header: q["header"].as_str().unwrap_or_default().into(),
                question,
                options: options
                    .iter()
                    .filter_map(|o| {
                        Some(QuestionOption {
                            label: o["label"].as_str()?.into(),
                            description: o["description"].as_str().unwrap_or_default().into(),
                        })
                    })
                    .collect(),
                multi_select: q["multiSelect"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

pub(super) fn claude_step_status(status: Option<&str>) -> StepStatus {
    match status {
        Some("in_progress") => StepStatus::Running,
        Some("completed") => StepStatus::Done,
        _ => StepStatus::Pending,
    }
}

/// The task type of a shell command. Its call returns as soon as the command is started, so the
/// call is shown finished then, and the command is listed under the background until it ends.
pub const SHELL_TASK: &str = "local_bash";

/// A task's status as a task patch names it. A task that has not ended is running, pending, or
/// paused; anything else has ended.
fn claude_task_status(status: &str) -> TaskStatus {
    match status {
        "running" => TaskStatus::Running,
        "pending" | "paused" => TaskStatus::Waiting,
        other => claude_ended_status(other),
    }
}

/// How an ended task is named: completed, failed, or killed (stopped by the user or its process).
fn claude_ended_status(status: &str) -> TaskStatus {
    match status {
        "completed" => TaskStatus::Done,
        "failed" => TaskStatus::Failed,
        _ => TaskStatus::Stopped,
    }
}

/// Task calls carry a `subagent_type`; that is what starts a subagent.
fn subagent_of(input: &Value) -> Option<SubAgent> {
    Some(SubAgent {
        kind: input["subagent_type"].as_str()?.into(),
        description: input["description"].as_str().unwrap_or_default().into(),
        model: input["model"].as_str().map(String::from),
        prompt: input["prompt"].as_str().map(String::from),
    })
}

fn claude_tool_use(
    block: &Value,
    root: &str,
    message_id: &str,
    parent: Option<&str>,
    out: &mut Vec<Parsed>,
) {
    let id = block["id"].as_str().unwrap_or_default();
    let name = block["name"].as_str().unwrap_or("Tool");
    let input = &block["input"];
    out.push(tool_started(
        message_id,
        ToolStart {
            id,
            name,
            summary: summarize(input, root),
            subagent: subagent_of(input),
            parent,
        },
    ));

    let path_of = |key: &str| input[key].as_str().map(|p| relative(root, p));
    let touched = match name {
        "Edit" | "Write" | "MultiEdit" => path_of("file_path"),
        "NotebookEdit" => path_of("notebook_path"),
        _ => None,
    };
    if let Some(path) = touched {
        out.push(file_touched(message_id, path));
    }
    if name == "TodoWrite" {
        let steps = input["todos"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|todo| PlanStep {
                text: todo["content"].as_str().unwrap_or_default().into(),
                status: claude_step_status(todo["status"].as_str()),
            })
            .collect();
        out.push(Parsed::Event(AgentEvent::PlanUpdated {
            message_id: message_id.into(),
            steps,
        }));
    }
}

/// The tokens of the model request an assistant message came from. Claude sends one message
/// per content block, and each carries its request's usage, so the request id tells them apart.
fn request_usage(message: &Value, top_level: bool) -> Option<Parsed> {
    let usage = message.get("usage")?;
    let count = |key: &str| usage[key].as_u64().unwrap_or(0);
    let input = count("input_tokens");
    let cache_read = count("cache_read_input_tokens");
    let cache_write = count("cache_creation_input_tokens");
    Some(Parsed::RequestUsage {
        request_id: message["id"].as_str()?.into(),
        usage: TokenUsage {
            input,
            cache_read,
            cache_write,
            output: count("output_tokens"),
            context: input + cache_read + cache_write,
        },
        top_level,
    })
}

/// Maps one line of `claude -p --output-format stream-json` output.
pub fn parse(v: &Value, root: &str, message_id: &str) -> Vec<Parsed> {
    let mut out = Vec::new();
    let blocks: &[Value] = v["message"]["content"]
        .as_array()
        .map_or(&[], Vec::as_slice);
    // Messages from inside a subagent name the Agent call they belong to.
    let parent = v["parent_tool_use_id"].as_str();
    match v["type"].as_str() {
        Some("system") if v["subtype"] == "task_started" => {
            if let (Some(tool_id), Some(task_id)) =
                (v["tool_use_id"].as_str(), v["task_id"].as_str())
            {
                let task = BackgroundTask {
                    tool_id: tool_id.into(),
                    task_id: Some(task_id.into()),
                    run_id: v["run_id"].as_str().map(String::from),
                    kind: v["task_type"].as_str().unwrap_or_default().into(),
                    description: v["description"].as_str().unwrap_or_default().into(),
                    status: TaskStatus::Running,
                    started_at: now_ms(),
                    summary: None,
                    output: None,
                };
                out.push(Parsed::Task(TaskEvent::Started {
                    task,
                    task_id: task_id.into(),
                    top_level: v["owned_by_subagent"] != true,
                    backgrounded: v["is_backgrounded"] == true,
                }));
            }
        }
        // A patch names the task by its top-level `task_id`. A status the task still has (running,
        // pending, paused) updates it; an ended one is reported as an end.
        Some("system") if v["subtype"] == "task_updated" => {
            if let Some(task_id) = v["task_id"].as_str() {
                let patch = &v["patch"];
                let status = patch["status"].as_str().map(claude_task_status);
                let description = patch["description"].as_str().map(String::from);
                let backgrounded = patch["is_backgrounded"] == true;
                let live = status.filter(|s| s.is_live());
                if backgrounded || description.is_some() || live.is_some() {
                    out.push(Parsed::Task(TaskEvent::Updated {
                        task_id: task_id.into(),
                        status: live,
                        description,
                        backgrounded,
                    }));
                }
                if let Some(status) = status.filter(|s| !s.is_live()) {
                    out.push(Parsed::Task(TaskEvent::Ended {
                        task_id: task_id.into(),
                        status,
                        summary: None,
                        output: None,
                    }));
                }
            }
        }
        // The progress line is the task's one-line text: `summary` when the CLI sends one, else its `description`.
        Some("system") if v["subtype"] == "task_progress" => {
            let summary = v["summary"].as_str().or_else(|| v["description"].as_str());
            if let (Some(task_id), Some(summary)) = (v["task_id"].as_str(), summary) {
                out.push(Parsed::Task(TaskEvent::Progress {
                    task_id: task_id.into(),
                    summary: summary.into(),
                }));
            }
        }
        Some("system") if v["subtype"] == "api_retry" && parent.is_none() => {
            let reason = if let Some(ms) = v["no_response"]["waited_ms"].as_u64() {
                format!("no response for {}s", ms / 1000)
            } else {
                match v["error"].as_str() {
                    Some("overloaded") => "API overloaded".into(),
                    Some("rate_limit") => "rate limited".into(),
                    Some("authentication_failed") => "authentication failed".into(),
                    _ => match v["error_status"].as_u64() {
                        Some(status) => format!("API error {status}"),
                        None => "API error".into(),
                    },
                }
            };
            let count = |key: &str| v[key].as_u64().unwrap_or(0) as u32;
            out.push(Parsed::Retrying {
                reason,
                attempt: count("attempt"),
                max_attempts: count("max_retries"),
            });
        }
        Some("rate_limit_event") => {
            let windows = crate::usage::claude::windows_from_event(&v["rate_limit_info"]);
            if !windows.is_empty() {
                out.push(Parsed::Usage(windows));
            }
        }
        Some("system") if v["subtype"] == "task_notification" => {
            if let Some(task_id) = v["task_id"].as_str() {
                // A task with no output, such as a subagent's own command, has an empty file name.
                let output = v["output_file"].as_str().filter(|path| !path.is_empty());
                out.push(Parsed::Task(TaskEvent::Ended {
                    task_id: task_id.into(),
                    status: claude_ended_status(v["status"].as_str().unwrap_or_default()),
                    summary: v["summary"].as_str().map(String::from),
                    output: output.map(String::from),
                }));
            }
        }
        Some("system") if v["subtype"] == "init" => {
            if let Some(id) = v["session_id"].as_str() {
                out.push(Parsed::SessionId(id.into()));
            }
            let commands = v["slash_commands"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|name| name.trim().trim_start_matches('/').to_owned())
                .filter(|name| !name.is_empty())
                .map(|name| ProviderSlashCommand {
                    name,
                    description: None,
                })
                .collect::<Vec<_>>();
            out.push(Parsed::SlashCommands(commands));
        }
        // Text arrives token by token in `stream_event`s; the full `assistant` text is skipped.
        Some("stream_event") => {
            let delta = &v["event"]["delta"];
            // A subagent's text is not part of the reply, so only the top level counts.
            if parent.is_none() && delta["type"] == "text_delta" {
                let text = delta["text"].as_str().unwrap_or_default();
                if !text.is_empty() {
                    out.push(text_delta(message_id, text));
                }
            }
            if parent.is_none() && delta["type"] == "thinking_delta" {
                let text = delta["thinking"].as_str().unwrap_or_default();
                if !text.is_empty() {
                    out.push(thinking(message_id, text));
                }
            }
            if parent.is_none()
                && v["event"]["type"] == "message_delta"
                && delta["stop_reason"] == "end_turn"
            {
                out.push(Parsed::Goal(GoalUpdate::Checking));
            }
        }
        // Command output such as `/goal` comes as a whole synthetic message, not streamed.
        Some("assistant") if parent.is_none() && v["message"]["model"] == "<synthetic>" => {
            let text = block_text(blocks);
            // A goal echoes the attachment list it was sent with; the message already shows them.
            let shown = without_attachments(&text);
            if !shown.is_empty() {
                out.push(text_delta(message_id, shown));
            }
            out.extend(goal_from_command(&text).map(Parsed::Goal));
        }
        Some("assistant") => {
            // A safety classifier stopped this reply. The CLI tells Claude so and the turn goes on,
            // so the turn does not fail; the reason is on the message.
            if parent.is_none() && v["message"]["stop_reason"] == "refusal" {
                let details = &v["message"]["stop_details"];
                out.push(Parsed::Event(AgentEvent::Refused {
                    message_id: message_id.into(),
                    refusal: Refusal {
                        category: details["category"].as_str().map(String::from),
                        explanation: details["explanation"]
                            .as_str()
                            .unwrap_or("Claude's response was stopped by a safety classifier.")
                            .into(),
                    },
                }));
            }
            for block in blocks {
                if block["type"] == "tool_use" {
                    claude_tool_use(block, root, message_id, parent, &mut out);
                }
            }
            out.extend(request_usage(&v["message"], parent.is_none()));
        }
        Some("user") => {
            out.extend(
                blocks
                    .iter()
                    .filter(|b| b["type"] == "tool_result")
                    .map(|block| tool_result(block, message_id)),
            );
            // Each `/goal` check that finds the goal unmet returns as Stop hook feedback.
            let content = &v["message"]["content"];
            let text = content
                .as_str()
                .map_or_else(|| block_text(blocks), String::from);
            if parent.is_none()
                && v["isSynthetic"] == true
                && text.starts_with("Stop hook feedback:\n[")
            {
                out.push(Parsed::Goal(GoalUpdate::Checked));
            }
        }
        Some("result") => {
            let is_error = v["is_error"].as_bool().unwrap_or(false);
            let subtype = v["subtype"].as_str().unwrap_or("error");
            // A steering message cuts the reply short; the agent goes on with it.
            let steered = v["terminal_reason"]
                .as_str()
                .is_some_and(|r| r.starts_with("aborted_"));
            if steered {
                out.push(Parsed::Event(AgentEvent::MessageCutOff {
                    message_id: message_id.into(),
                }));
            }
            // The CLI can end a turn that failed without flagging it; its reason says so.
            let reason_error = v["terminal_reason"].as_str().and_then(terminal_error);
            if (is_error || subtype != "success" || reason_error.is_some()) && !steered {
                // Failures before the model runs, like a session it cannot find, come in `errors`.
                let errors = v["errors"].as_array().map_or(&[][..], Vec::as_slice);
                let listed = errors
                    .iter()
                    .filter_map(Value::as_str)
                    .find(|e| !e.starts_with("[ede_diagnostic]"));
                if listed.is_some_and(|e| e.starts_with("No conversation found with session ID")) {
                    out.push(Parsed::SessionLost);
                }
                let text = v["result"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or(listed)
                    .or(reason_error)
                    .unwrap_or(subtype);
                out.push(Parsed::Error(text.into()));
            }
            out.push(Parsed::Finished);
        }
        // In plan mode Claude asks to leave it once its plan is ready. The plan is kept and the
        // request refused, so Claude stops and the user implements or refines it.
        Some("control_request")
            if v["request"]["subtype"] == "can_use_tool"
                && v["request"]["tool_name"] == "ExitPlanMode" =>
        {
            let plan = v["request"]["input"]["plan"]
                .as_str()
                .unwrap_or_default()
                .trim();
            if !plan.is_empty() {
                out.push(Parsed::Event(AgentEvent::PlanProposed {
                    message_id: message_id.into(),
                    text: plan.into(),
                }));
            }
            out.push(Parsed::Send(
                serde_json::json!({
                    "type": "control_response",
                    "response": {
                        "subtype": "success",
                        "request_id": v["request_id"],
                        "response": {"behavior": "deny", "message": PLAN_CAPTURED},
                    },
                })
                .to_string(),
            ));
        }
        Some("control_request") => out.push(claude_control_request(v, root)),
        Some("control_response") => {
            if let Some(id) = v["response"]["request_id"].as_str() {
                out.push(Parsed::ControlResponse(id.into()));
            }
        }
        _ => {}
    }
    out
}

/// What went wrong, for a `terminal_reason` that ends a turn as failed.
fn terminal_error(reason: &str) -> Option<&'static str> {
    const CONTEXT_FULL: &str = "Claude stopped: the prompt exceeds the model's context window. Run /compact or start a new task.";
    Some(match reason {
        "api_error" => "Claude gave up after repeated API errors.",
        "malformed_tool_use_exhausted" => "Claude gave up after repeated malformed tool calls.",
        "budget_exhausted" => "Claude stopped: the turn's token budget ran out.",
        "structured_output_retry_exhausted" => {
            "Claude could not produce the requested structured output."
        }
        "tool_deferred_unavailable" => {
            "Claude could not resume a tool call: the tool is no longer available."
        }
        "turn_setup_failed" => "Claude could not start the turn.",
        // The CLI also reports a full context window this way.
        "blocking_limit" | "prompt_too_long" => CONTEXT_FULL,
        "rapid_refill_breaker" => {
            "Claude stopped: the context refilled too quickly after compaction."
        }
        "image_error" => "Claude stopped: an image in the conversation could not be processed.",
        "model_error" => "Claude stopped: the model returned an error.",
        _ => return None,
    })
}

fn block_text(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
}

/// Reads Claude's `/goal` command output: "Goal set: <objective>",
/// "Goal active: <objective> (3 turns)", or that the goal was cleared.
fn goal_from_command(text: &str) -> Option<GoalUpdate> {
    if let Some(objective) = text.strip_prefix("Goal set: ") {
        return Some(GoalUpdate::Set {
            objective: without_attachments(objective).trim().into(),
            checks: Some(0),
        });
    }
    if text.starts_with("Goal cleared") || text.starts_with("No goal set") {
        return Some(GoalUpdate::Cleared);
    }
    let rest = text.strip_prefix("Goal active: ")?;
    let line = rest.lines().next().unwrap_or(rest);
    // A goal that names attachments spans several lines, with the turn count after the list.
    let (objective, count) = match line.rsplit_once(" (") {
        Some(split) => split,
        None => (without_attachments(rest), rest.rsplit_once(" (")?.1),
    };
    let checks = count
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    Some(GoalUpdate::Set {
        objective: objective.into(),
        checks: Some(checks),
    })
}

/// Where Claude's goal stands once its run has ended. The stream does not say whether the
/// evaluator found the goal met, judged it impossible, or gave up on a loop that made no
/// progress, so this reads the evaluator's last verdict from the session transcript.
pub fn goal_outcome(session_id: &str, objective: &str) -> GoalStatus {
    let file = format!("{session_id}.jsonl");
    let transcript = home()
        .and_then(|h| std::fs::read_dir(h.join("projects")).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|d| d.path().join(&file))
        .find(|p| p.is_file())
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    // Setting the goal writes a `sentinel` entry; each verdict after it writes another.
    let verdict = transcript
        .lines()
        .rev()
        .filter(|l| l.contains("\"goal_status\""))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|v| v["attachment"].clone())
        .find(|a| {
            a["type"] == "goal_status"
                && a["condition"].as_str().map(without_attachments) == Some(objective)
        });
    match verdict {
        Some(a) if a["met"] == true => GoalStatus::Complete,
        Some(a) if a["failed"] == true => GoalStatus::Blocked,
        // Unmet or not yet judged: the goal is still set in the session and goes on
        // with the next message.
        _ => GoalStatus::Paused,
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    // Lines the CLI sent, captured from Claude Code 2.1.310 on a Haiku turn: a background command, a
    // background subagent with its own command, and a foreground command moved to the background.
    // Task lines are verbatim; the tool results and the turn result are trimmed to the fields the
    // parser reads.
    pub const BG_SHELL_STARTED: &str = r#"{"type":"system","subtype":"task_started","task_id":"bsktq4olb","run_id":"0mvsny618-461e82c2","tool_use_id":"toolu_01UcFx54BgAVg64YDxNNvZHh","description":"Run a short sleep in the background and echo a marker","is_backgrounded":true,"task_type":"local_bash","uuid":"5a0ad893-e218-4031-b435-641d1c677cda","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
    pub const BG_SHELL_UPDATED: &str = r#"{"type":"system","subtype":"task_updated","task_id":"bsktq4olb","run_id":"0mvsny618-461e82c2","patch":{"status":"completed","end_time":1793225143763},"uuid":"55c176bd-ee9b-4e94-9d7c-852caf9d7e30","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
    pub const BG_SHELL_NOTIFIED: &str = r#"{"type":"system","subtype":"task_notification","task_id":"bsktq4olb","run_id":"0mvsny618-461e82c2","tool_use_id":"toolu_01UcFx54BgAVg64YDxNNvZHh","status":"completed","output_file":"/home/polaris/Projects/PatchworkADE/research/claude-capture/tmp/claude-1000/-home-polaris-Projects-PatchworkADE-research-claude-capture/4d035a83-ccf0-4211-bc1a-8e16e2e5cd55/tasks/bsktq4olb.output","summary":"Background command \"Run a short sleep in the background and echo a marker\" completed (exit code 0)","uuid":"b2d4b658-92ea-4b5c-9e96-deaa6d7c9425","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
    pub const AGENT_STARTED: &str = r#"{"type":"system","subtype":"task_started","task_id":"adf0968466f0fada1","run_id":"0mvsny6bc-f97bc88a","tool_use_id":"toolu_01GQWb3a5rvJrVhxJtrPz2BN","description":"Haiku sleep check","subagent_type":"general-purpose","is_backgrounded":true,"spawn_depth":1,"task_type":"local_agent","prompt":"Run sleep 4 with Bash and reply OK.","uuid":"e0045c4e-625e-4882-9bd3-1f980726596b","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
    pub const AGENT_PROGRESS: &str = r#"{"type":"system","subtype":"task_progress","task_id":"adf0968466f0fada1","run_id":"0mvsny6bc-f97bc88a","tool_use_id":"toolu_01GQWb3a5rvJrVhxJtrPz2BN","description":"Running Sleep for 4 seconds","subagent_type":"general-purpose","usage":{"total_tokens":20624,"tool_uses":1,"duration_ms":2155},"last_tool_name":"Bash","uuid":"913f4847-1809-46dd-b614-c3ca458f0e4e","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
    pub const OWNED_SHELL_STARTED: &str = r#"{"type":"system","subtype":"task_started","task_id":"btmpqjc76","run_id":"0mvsnyaam-40cbdd66","owned_by_subagent":true,"tool_use_id":"toolu_017nYGv4WtxfVr2PMXvRukas","description":"Sleep for 4 seconds","is_backgrounded":false,"parent_task_id":"adf0968466f0fada1","task_type":"local_bash","uuid":"9e33aea8-1a5a-41b3-a8f5-1dd6b02e2bb5","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
    pub const OWNED_SHELL_NOTIFIED: &str = r#"{"type":"system","subtype":"task_notification","task_id":"btmpqjc76","run_id":"0mvsnyaam-40cbdd66","tool_use_id":"toolu_017nYGv4WtxfVr2PMXvRukas","status":"completed","output_file":"","summary":"Sleep for 4 seconds","uuid":"a7d7c6a2-f384-43a8-b1d2-e3a901ec2a37","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
    pub const AGENT_UPDATED: &str = r#"{"type":"system","subtype":"task_updated","task_id":"adf0968466f0fada1","run_id":"0mvsny6bc-f97bc88a","patch":{"status":"completed","end_time":1793225148310},"uuid":"2365cd5c-0c13-4d93-b120-317608496eee","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
    pub const AGENT_NOTIFIED: &str = r#"{"type":"system","subtype":"task_notification","task_id":"adf0968466f0fada1","run_id":"0mvsny6bc-f97bc88a","tool_use_id":"toolu_01GQWb3a5rvJrVhxJtrPz2BN","status":"completed","output_file":"/home/polaris/Projects/PatchworkADE/research/claude-capture/tmp/claude-1000/-home-polaris-Projects-PatchworkADE-research-claude-capture/4d035a83-ccf0-4211-bc1a-8e16e2e5cd55/tasks/adf0968466f0fada1.output","summary":"OK","usage":{"total_tokens":21288,"tool_uses":1,"duration_ms":7201},"uuid":"adbb5608-381d-4695-9154-a9cb5bb28d6e","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
    pub const AGENT_LAUNCHED: &str = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01GQWb3a5rvJrVhxJtrPz2BN","content":"Async agent launched successfully."}]},"parent_tool_use_id":null}"#;
    pub const TURN_RESULT: &str =
        r#"{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}"#;
    pub const PROMOTED_STARTED: &str = r#"{"type":"system","subtype":"task_started","task_id":"ba28p82m4","run_id":"0mvso3an8-e7937c61","tool_use_id":"toolu_015w8zbEatreDdpozAygHZzs","description":"node -e \"setTimeout(() => console.log(\\\"fg-done\\\"), 125000)\"","is_backgrounded":false,"task_type":"local_bash","uuid":"ce414b27-8968-4b7c-a054-5a3788ce0f94","session_id":"b9bacc24-0d88-4c7d-a678-d532362bc092"}"#;
    pub const PROMOTED_UPDATED: &str = r#"{"type":"system","subtype":"task_updated","task_id":"ba28p82m4","run_id":"0mvso3an8-e7937c61","patch":{"is_backgrounded":true},"uuid":"53776972-365c-4e1b-8c4d-1012cf2a8741","session_id":"b9bacc24-0d88-4c7d-a678-d532362bc092"}"#;
    pub const PROMOTED_ENDED: &str = r#"{"type":"system","subtype":"task_updated","task_id":"ba28p82m4","run_id":"0mvso3an8-e7937c61","patch":{"status":"completed","end_time":1793225502117},"uuid":"45b55761-b304-4b7e-80ae-f3c7a874ff90","session_id":"b9bacc24-0d88-4c7d-a678-d532362bc092"}"#;
    pub const PROMOTED_NOTIFIED: &str = r#"{"type":"system","subtype":"task_notification","task_id":"ba28p82m4","run_id":"0mvso3an8-e7937c61","tool_use_id":"toolu_015w8zbEatreDdpozAygHZzs","status":"completed","output_file":"/home/polaris/Projects/PatchworkADE/research/claude-capture/tmp/claude-1000/-home-polaris-Projects-PatchworkADE-research-claude-capture/b9bacc24-0d88-4c7d-a678-d532362bc092/tasks/ba28p82m4.output","summary":"Background command \"node -e \"setTimeout(() => console.log(\\\"fg-done\\\"), 125000)\"\" completed (exit code 0)","uuid":"8eaaeb7f-2cb4-4ab4-9245-47379b5e8f61","session_id":"b9bacc24-0d88-4c7d-a678-d532362bc092"}"#;
    pub const PROMOTED_RESULT: &str = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_015w8zbEatreDdpozAygHZzs","content":"Command did not complete within its 120s timeout and was moved to the background (ID: ba28p82m4)."}]},"parent_tool_use_id":null}"#;
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::fixtures::AGENT_PROGRESS;
    use super::fixtures::BG_SHELL_NOTIFIED;
    use super::fixtures::BG_SHELL_STARTED;
    use super::fixtures::BG_SHELL_UPDATED;
    use super::fixtures::OWNED_SHELL_NOTIFIED;
    use super::fixtures::OWNED_SHELL_STARTED;
    use super::fixtures::PROMOTED_ENDED;
    use super::fixtures::PROMOTED_NOTIFIED;
    use super::fixtures::PROMOTED_STARTED;
    use super::fixtures::PROMOTED_UPDATED;
    use serde_json::json;

    #[test]
    fn claude_init_is_session_and_noise_is_ignored() {
        let init =
            json!({"type": "system", "subtype": "init", "cwd": "/abs", "session_id": "9578"});
        let out = parse(&init, "/abs", "m1");
        assert!(
            out.iter()
                .any(|item| matches!(item, Parsed::SessionId(id) if id == "9578"))
        );
        assert!(
            out.iter()
                .any(|item| matches!(item, Parsed::SlashCommands(commands) if commands.is_empty()))
        );

        let hook = json!({"type": "system", "subtype": "hook_started"});
        assert!(parse(&hook, "/abs", "m1").is_empty());
        let rate = json!({"type": "rate_limit_event"});
        assert!(parse(&rate, "/abs", "m1").is_empty());
    }

    #[test]
    fn init_advertises_native_slash_commands() {
        let init = json!({"type":"system","subtype":"init","session_id":"s","slash_commands":["compact","review"]});
        let parsed = parse(&init, "/", "m");
        assert!(parsed.iter().any(|item| matches!(item, Parsed::SlashCommands(commands)
            if commands.iter().map(|command| command.name.as_str()).collect::<Vec<_>>() == ["compact", "review"])));
    }

    #[test]
    fn claude_assistant_text_and_edit() {
        let v = json!({"type": "assistant", "message": {"content": [
            {"type": "text", "text": "hi"},
            {"type": "thinking", "thinking": "..."},
            {"type": "tool_use", "id": "toolu_1", "name": "Edit",
             "input": {"file_path": "/abs/a.css", "old_string": "..", "new_string": ".."}}
        ]}});
        let out = parse(&v, "/abs", "m1");
        // The text block was already streamed as deltas, so only the tool calls remain.
        assert_eq!(out.len(), 2);
        assert!(matches!(
            &out[0],
            Parsed::Event(AgentEvent::ToolStarted { tool, .. })
                if tool.id == "toolu_1" && tool.name == "Edit" && tool.summary == "a.css"
                    && tool.status == StepStatus::Running
        ));
        assert!(
            matches!(&out[1], Parsed::Event(AgentEvent::FileTouched { path, .. }) if path == "a.css")
        );
    }

    #[test]
    fn claude_todo_write_becomes_plan() {
        let v = json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": "t2", "name": "TodoWrite", "input": {"todos": [
                {"content": "Plan", "status": "completed"},
                {"content": "Build", "status": "in_progress"},
                {"content": "Ship", "status": "pending"}
            ]}}
        ]}});
        let out = parse(&v, "/abs", "m1");
        let [_, Parsed::Event(AgentEvent::PlanUpdated { steps, .. })] = out.as_slice() else {
            panic!("expected a plan update, got {out:?}");
        };
        let statuses: Vec<StepStatus> = steps.iter().map(|s| s.status).collect();
        assert_eq!(
            statuses,
            [StepStatus::Done, StepStatus::Running, StepStatus::Pending]
        );
    }

    #[test]
    fn claude_tool_result_and_result_errors() {
        let user = json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "toolu_1", "is_error": true, "content": "x"}
        ]}});
        let out = parse(&user, "/abs", "m1");
        assert!(matches!(
            out.as_slice(),
            [Parsed::Event(AgentEvent::ToolFinished { tool_id, is_error: true, .. })] if tool_id == "toolu_1"
        ));

        let failed = json!({"type": "result", "subtype": "success", "is_error": true, "result": "Invalid key"});
        let out = parse(&failed, "/abs", "m1");
        assert!(
            matches!(out.as_slice(), [Parsed::Error(e), Parsed::Finished] if e == "Invalid key")
        );

        let ok = json!({"type": "result", "subtype": "success", "is_error": false, "result": "hi"});
        assert!(matches!(
            parse(&ok, "/abs", "m1").as_slice(),
            [Parsed::Finished]
        ));
    }

    #[test]
    fn claude_permission_request_round_trip() {
        let request = json!({"type": "control_request", "request_id": "r1", "request": {
            "subtype": "can_use_tool", "tool_name": "Bash", "display_name": "Bash",
            "input": {"command": "rm -rf build"}}});
        let out = parse(&request, "/abs", "m1");
        let [
            Parsed::Approval {
                title,
                detail,
                reply,
                ..
            },
        ] = out.as_slice()
        else {
            panic!("{out:?}");
        };
        assert_eq!(
            (title.as_str(), detail.as_str()),
            ("Run command", "rm -rf build")
        );

        let allow: Value =
            serde_json::from_str(&approval_reply(reply, true, &HashMap::new())).unwrap();
        assert_eq!(allow["response"]["request_id"], "r1");
        assert_eq!(allow["response"]["response"]["behavior"], "allow");
        assert_eq!(
            allow["response"]["response"]["updatedInput"]["command"],
            "rm -rf build"
        );
        let deny: Value =
            serde_json::from_str(&approval_reply(reply, false, &HashMap::new())).unwrap();
        assert_eq!(deny["response"]["response"]["behavior"], "deny");

        let other = json!({"type": "control_request", "request_id": "r2",
            "request": {"subtype": "hook_callback"}});
        let out = parse(&other, "/abs", "m1");
        assert!(matches!(out.as_slice(), [Parsed::Send(line)] if line.contains("\"error\"")));
    }

    #[test]
    fn claude_args_include_resume_model_and_mode() {
        let mut spec = TurnSpec {
            root: "/abs",
            session_id: Some("sid"),
            model: Some("opus"),
            vision_model: None,
            text: "do it",
            mode: AccessMode::FullAccess,
            plan: false,
            effort: None,
            attachments: &[],
            images: &[],
            instructions: None,
            skills_dir: None,
            guarded: &[],
            agents: &[],
        };
        assert_eq!(
            turn_args(&spec),
            [
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--forward-subagent-text",
                "--thinking-display",
                "summarized",
                "--permission-mode",
                "bypassPermissions",
                "--permission-prompt-tool",
                "stdio",
                "--resume",
                "sid",
                "--model",
                "opus",
            ]
        );
        spec.mode = AccessMode::Supervised;
        let args = turn_args(&spec);
        assert!(
            args.windows(2)
                .any(|w| w == ["--permission-mode", "manual"])
        );
        assert!(
            args.windows(2)
                .any(|w| w == ["--permission-prompt-tool", "stdio"])
        );
    }

    #[test]
    fn a_model_takes_the_levels_its_catalog_entry_has() {
        let catalog = vec![
            ClaudeModel {
                id: "claude-opus-5-5".into(),
                name: "Opus 5.5".into(),
                context_window: Some(1_800_000),
                efforts: vec!["low".into(), "medium".into(), "xhigh".into()],
                fast_mode: true,
            },
            ClaudeModel {
                id: "claude-haiku-4-5-20251001".into(),
                name: "Haiku 4.5".into(),
                context_window: Some(200_000),
                efforts: Vec::new(),
                fast_mode: false,
            },
        ];
        assert_eq!(
            efforts_in(&catalog, "claude-opus-5-5"),
            Some(vec!["low".into(), "medium".into(), "xhigh".into()])
        );
        // A pinned context window is not part of the id.
        assert!(efforts_in(&catalog, "claude-opus-5-5[1m]").is_some());
        // A dated snapshot of a listed model matches it, and takes none when it has none.
        assert_eq!(efforts_in(&catalog, "claude-haiku-4-5"), Some(Vec::new()));
        // A later model does not take the levels of the one its id starts with.
        assert_eq!(efforts_in(&catalog, "claude-opus-5"), None);
        assert_eq!(efforts_in(&catalog, "claude-sonnet-4-6"), None);
    }

    fn parsed(line: &str) -> Vec<Parsed> {
        parse(&serde_json::from_str(line).unwrap(), "/abs", "m1")
    }

    #[test]
    fn a_background_command_starts_listed_with_its_identity() {
        let out = parsed(BG_SHELL_STARTED);
        let [
            Parsed::Task(TaskEvent::Started {
                task,
                task_id,
                top_level,
                backgrounded,
            }),
        ] = out.as_slice()
        else {
            panic!("expected one started task, got {out:?}");
        };
        assert_eq!(task_id.as_str(), "bsktq4olb");
        assert!(*top_level && *backgrounded);
        assert_eq!(task.tool_id, "toolu_01UcFx54BgAVg64YDxNNvZHh");
        assert_eq!(task.task_id.as_deref(), Some("bsktq4olb"));
        assert_eq!(task.run_id.as_deref(), Some("0mvsny618-461e82c2"));
        assert_eq!(
            (task.kind.as_str(), task.status),
            ("local_bash", TaskStatus::Running)
        );
        assert!(task.started_at > 0);
    }

    #[test]
    fn a_status_patch_ends_the_task_and_its_notification_adds_the_output() {
        let out = parsed(BG_SHELL_UPDATED);
        assert!(matches!(
            out.as_slice(),
            [Parsed::Task(TaskEvent::Ended { task_id, status: TaskStatus::Done, summary: None, output: None })]
                if task_id.as_str() == "bsktq4olb"
        ));

        let out = parsed(BG_SHELL_NOTIFIED);
        let [
            Parsed::Task(TaskEvent::Ended {
                status: TaskStatus::Done,
                summary: Some(summary),
                output: Some(output),
                ..
            }),
        ] = out.as_slice()
        else {
            panic!("expected the notification to carry the output, got {out:?}");
        };
        assert!(summary.starts_with("Background command"));
        assert!(output.ends_with("/tasks/bsktq4olb.output"));
    }

    #[test]
    fn a_subagents_own_command_is_not_top_level_and_has_no_output_file() {
        let out = parsed(OWNED_SHELL_STARTED);
        assert!(matches!(
            out.as_slice(),
            [Parsed::Task(TaskEvent::Started {
                top_level: false,
                backgrounded: false,
                ..
            })]
        ));
        // The CLI sends an empty output file name for it, which is no output.
        let out = parsed(OWNED_SHELL_NOTIFIED);
        assert!(matches!(
            out.as_slice(),
            [Parsed::Task(TaskEvent::Ended {
                status: TaskStatus::Done,
                output: None,
                ..
            })]
        ));
    }

    #[test]
    fn a_subagents_progress_updates_it_and_never_ends_it() {
        // This capture names the progress text `description`, with no `summary`.
        let out = parsed(AGENT_PROGRESS);
        assert!(matches!(
            out.as_slice(),
            [Parsed::Task(TaskEvent::Progress { task_id, summary })]
                if task_id.as_str() == "adf0968466f0fada1" && summary.as_str() == "Running Sleep for 4 seconds"
        ));
    }

    #[test]
    fn a_foreground_command_moved_to_the_background_is_listed_by_the_patch() {
        let out = parsed(PROMOTED_STARTED);
        assert!(matches!(
            out.as_slice(),
            [Parsed::Task(TaskEvent::Started {
                top_level: true,
                backgrounded: false,
                ..
            })]
        ));
        let out = parsed(PROMOTED_UPDATED);
        assert!(matches!(
            out.as_slice(),
            [Parsed::Task(TaskEvent::Updated { task_id, status: None, description: None, backgrounded: true })]
                if task_id.as_str() == "ba28p82m4"
        ));
        let out = parsed(PROMOTED_ENDED);
        assert!(matches!(
            out.as_slice(),
            [Parsed::Task(TaskEvent::Ended {
                status: TaskStatus::Done,
                output: None,
                ..
            })]
        ));
        // Its notification comes after the status, and names the file the output went to.
        let out = parsed(PROMOTED_NOTIFIED);
        assert!(matches!(
            out.as_slice(),
            [Parsed::Task(TaskEvent::Ended {
                status: TaskStatus::Done,
                summary: Some(_),
                output: Some(_),
                ..
            })]
        ));
    }

    #[test]
    fn task_statuses_map_to_live_or_ended() {
        assert_eq!(claude_task_status("running"), TaskStatus::Running);
        assert_eq!(claude_task_status("pending"), TaskStatus::Waiting);
        assert_eq!(claude_task_status("paused"), TaskStatus::Waiting);
        assert_eq!(claude_ended_status("completed"), TaskStatus::Done);
        assert_eq!(claude_ended_status("failed"), TaskStatus::Failed);
        assert_eq!(claude_ended_status("killed"), TaskStatus::Stopped);
    }
}
