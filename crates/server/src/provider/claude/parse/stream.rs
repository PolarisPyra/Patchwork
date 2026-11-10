//! Reading one stream-json line into agent events.

// Rust guideline compliant 2026-10-09

use serde_json::Value;

use crate::events::AgentEvent;
use crate::threads::BackgroundTask;
use crate::system::now_ms;
use crate::threads::PlanStep;
use crate::provider::ProviderSlashCommand;
use crate::provider::step_status;
use crate::threads::Refusal;
use crate::threads::TaskStatus;
use crate::threads::TokenUsage;
use crate::provider::edit_lines;
use crate::provider::file_touched;
use crate::provider::GoalUpdate;
use crate::provider::without_attachments;
use crate::provider::Parsed;
use crate::provider::TaskEvent;
use crate::provider::relative;
use crate::provider::summarize;
use crate::provider::text_delta;
use crate::provider::thinking;
use crate::provider::tool_started;
use crate::provider::ToolStart;

use super::super::PLAN_CAPTURED;
use super::goal::goal_from_command;
use super::requests::claude_control_request;
use super::tasks::{claude_ended_status, claude_task_status, subagent_of, tool_result};

pub(super) fn claude_tool_use(
    block: &Value,
    root: &str,
    message_id: &str,
    parent: Option<&str>,
    out: &mut Vec<Parsed>,
) {
    let id = block["id"].as_str().unwrap_or_default();
    let name = block["name"].as_str().unwrap_or("Tool");
    let input = &block["input"];
    let mut started = tool_started(
        message_id,
        ToolStart {
            id,
            name,
            summary: summarize(input, root),
            subagent: subagent_of(input),
            parent,
        },
    );
    if let Parsed::Event(AgentEvent::ToolStarted { tool, .. }) = &mut started {
        tool.lines = edit_lines(name, input);
    }
    out.push(started);

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
                status: step_status(todo["status"].as_str()),
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
            ..Default::default()
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
        Some("system")
            if v["subtype"] == "status" && v["status"] == "compacting" && parent.is_none() =>
        {
            out.push(Parsed::Compacting);
        }
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
        // The full set of live tasks, which the CLI sends whenever it changes with REPLACE
        // semantics. It is the only report that survives a lost end, so a task it leaves out has
        // ended. An entry with no string id, or one marked ambient, is not a task the strip shows.
        Some("system") if v["subtype"] == "background_tasks_changed" => {
            let live = v["tasks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|task| task["ambient"] != true)
                .filter_map(|task| task["task_id"].as_str())
                .map(String::from)
                .collect();
            out.push(Parsed::Task(TaskEvent::Level { live }));
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

pub(super) fn block_text(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
}
