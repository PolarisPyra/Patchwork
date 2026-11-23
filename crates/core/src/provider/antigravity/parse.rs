//! Maps JSON-RPC messages from `agy acp` into Patchwork agent events and approvals.

// Rust guideline compliant 2026-11-22

use std::path::Path;

use serde_json::{Value, json};

use crate::chats::{BackgroundTask, PlanStep, Refusal, TaskStatus, TokenUsage, ToolCall};
use crate::system::now_ms;
use crate::events::AgentEvent;
use crate::permissions::{PermissionKind, Target};
use crate::provider::{
    Parsed, ProviderSlashCommand, ToolStart, TurnSpec, file_touched, one_line, relative,
    step_status, summarize, text_delta, thinking, tool_finished, tool_started,
};

use super::client::AUTH_REQUIRED_CODE;
use super::input::{
    EFFORT_REQUEST, INIT_REQUEST, MODE_REQUEST, MODEL_REQUEST, PROMPT_REQUEST, SESSION_REQUEST,
    begin,
};
use super::swarm::{extract_child_target, format_swarm_tool, invoke_subagent_descriptor};

/// Maps one JSON-RPC line from `agy acp` into Patchwork events, approvals, or follow-up writes.
#[must_use]
pub fn parse(v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
    let mut out = Vec::new();
    match (v.get("id"), v["method"].as_str()) {
        // Response to one of our JSON-RPC requests.
        (Some(id), None) => handle_response(id, v, spec, message_id, &mut out),
        // Server-initiated request (such as `session/request_permission`).
        (Some(_), Some(_)) => out.push(handle_server_request(v, spec)),
        // Server notification (`session/update`).
        (None, Some("session/update")) => {
            parse_session_update(&v["params"]["update"], spec, message_id, &mut out);
        }
        _ => {}
    }
    out
}

/// The task a shell command call runs, listed under the background when the reply ends while
/// it still runs: Antigravity answers its prompt once a long command moves to the background.
#[must_use]
pub fn background_task(tool: &ToolCall) -> Option<BackgroundTask> {
    if !matches!(
        tool.name.as_str(),
        "run_command" | "execute_command" | "shell" | "execute"
    ) {
        return None;
    }
    Some(BackgroundTask {
        tool_id: tool.id.clone(),
        task_id: None,
        run_id: None,
        kind: "command".into(),
        description: tool.summary.clone(),
        status: TaskStatus::Running,
        started_at: tool.started_at.unwrap_or_else(now_ms),
        summary: None,
        output: None,
        cut_off: false,
    })
}

/// Whether `line` shows Antigravity at work: text, a thought, a plan, or a call running.
///
/// It sends these too when it goes on working after answering its prompt. A call that only
/// ends, such as a background command, is not work.
#[must_use]
pub fn is_work(line: &Value) -> bool {
    let update = &line["params"]["update"];
    let ends_call = matches!(update["status"].as_str(), Some("completed" | "failed"));
    line.get("id").is_none()
        && line["method"] == "session/update"
        && match update["sessionUpdate"].as_str() {
            Some("agent_message_chunk" | "agent_thought_chunk" | "plan") => true,
            Some("tool_call" | "tool_call_update") => !ends_call,
            _ => false,
        }
}

fn handle_response(
    id: &Value,
    v: &Value,
    spec: &TurnSpec,
    message_id: &str,
    out: &mut Vec<Parsed>,
) {
    let error = v.get("error").filter(|e| !e.is_null());
    let error_msg = error.and_then(|e| e["message"].as_str());
    let error_code = error.and_then(|e| e["code"].as_i64());

    if id == INIT_REQUEST {
        if let Some(msg) = error_msg {
            out.extend([Parsed::Error(msg.into()), Parsed::Finished]);
        }
        return;
    }

    if id == SESSION_REQUEST {
        if let Some(msg) = error_msg {
            if error_code == Some(AUTH_REQUIRED_CODE)
                || msg.to_lowercase().contains("authentication")
            {
                out.extend([
                    Parsed::Error(
                        "Antigravity authentication required. Sign in under Settings → Providers."
                            .into(),
                    ),
                    Parsed::Finished,
                ]);
            } else if spec.session_id.is_some() && session_missing(msg) {
                // A session agy no longer has is started afresh, given the conversation so far.
                // Other failures are shown, so a passing one does not replace the session.
                out.extend([Parsed::SessionLost, Parsed::Finished]);
            } else {
                out.extend([Parsed::Error(msg.into()), Parsed::Finished]);
            }
            return;
        }
        if let Some(session_id) = v["result"]["sessionId"]
            .as_str()
            .filter(|id| !id.is_empty())
        {
            out.push(Parsed::SessionId(session_id.into()));
            out.extend(begin(spec, session_id, &v["result"]));
        } else {
            out.extend([
                Parsed::Error("Antigravity ACP did not return a session id".into()),
                Parsed::Finished,
            ]);
        }
        return;
    }

    if id == MODE_REQUEST || id == EFFORT_REQUEST {
        // Older or restricted models may reject optional mode/thought_level tweaks; keep going.
        return;
    }

    if id == MODEL_REQUEST {
        if let Some(msg) = error_msg {
            out.extend([Parsed::Error(msg.into()), Parsed::Finished]);
        }
        return;
    }

    if id == PROMPT_REQUEST {
        if let Some(msg) = error_msg {
            out.extend([Parsed::Error(msg.into()), Parsed::Finished]);
            return;
        }
        let result = &v["result"];
        let usage = &result["usage"];
        if usage.is_object() {
            let count = |key: &str| usage[key].as_u64().unwrap_or(0);
            let input_tokens = count("inputTokens");
            let cached_read = count("cachedReadTokens");
            let cached_write = count("cachedWriteTokens");
            let output_tokens = count("outputTokens") + count("thoughtTokens");
            let total_tokens = count("totalTokens");
            if input_tokens > 0 || output_tokens > 0 || total_tokens > 0 {
                out.push(Parsed::RequestUsage {
                    request_id: format!("acp-prompt-{total_tokens}"),
                    usage: TokenUsage {
                        input: input_tokens.saturating_sub(cached_read),
                        cache_read: cached_read,
                        cache_write: cached_write,
                        output: output_tokens,
                        context: if input_tokens > 0 {
                            input_tokens
                        } else {
                            total_tokens
                        },
                        ..Default::default()
                    },
                    top_level: true,
                    model: None,
                });
            }
        }
        out.extend(stop_reason(result["stopReason"].as_str(), message_id));
        out.push(Parsed::Finished);
    }
}

/// Whether a `session/resume` error says agy has no such session. agy 1.5 answers an unknown
/// id with `Failed to read session <id>; retry, or load it with session/load`.
fn session_missing(message: &str) -> bool {
    let lower = message.to_lowercase();
    [
        "failed to read session",
        "not found",
        "no such session",
        "does not exist",
    ]
    .iter()
    .any(|term| lower.contains(term))
}

/// The usage-limit error agy writes into the reply as text, such as `⚠ Individual quota
/// reached. … Resets in 1h12m40s.`, before ending it as a normal reply.
fn limit_notice(text: &str) -> Option<&str> {
    let notice = text
        .trim()
        .strip_prefix('⚠')?
        .trim_start_matches('\u{fe0f}')
        .trim();
    let lower = notice.to_lowercase();
    [
        "quota",
        "resource_exhausted",
        "429",
        "exhausted your capacity",
        "rate limit",
    ]
    .iter()
    .any(|term| lower.contains(term))
    .then_some(notice)
}

/// What a reply's ACP `stopReason` adds before it ends: nothing for a normal end or a stop.
fn stop_reason(reason: Option<&str>, message_id: &str) -> Option<Parsed> {
    let error = match reason? {
        "refusal" => {
            return Some(Parsed::Event(AgentEvent::Refused {
                message_id: message_id.into(),
                refusal: Refusal {
                    category: None,
                    explanation: "Antigravity declined to continue this reply.".into(),
                },
            }));
        }
        "max_tokens" => "Antigravity's reply reached the model's output limit and was cut off.",
        "max_turn_requests" => {
            "Antigravity reached its limit of model requests for one reply. Send a message to \
             continue."
        }
        _ => return None,
    };
    Some(Parsed::Error(error.into()))
}

fn handle_server_request(v: &Value, spec: &TurnSpec) -> Parsed {
    if v["method"].as_str() != Some("session/request_permission") {
        return Parsed::Send(
            json!({
                "jsonrpc": "2.0",
                "id": v["id"],
                "error": {"code": -32601, "message": "Method not supported by Patchwork"},
            })
            .to_string(),
        );
    }

    let params = &v["params"];
    let tool = &params["toolCall"];
    let options = params["options"].as_array().map_or(&[][..], Vec::as_slice);

    // Patchwork asks about each call, so a one-off answer is preferred; a lasting one only
    // when agy offers nothing else.
    let kind = |kind: &str| {
        options
            .iter()
            .find(move |o| o["kind"].as_str() == Some(kind))
    };
    let allow_option = kind("allow_once")
        .or_else(|| kind("allow_always"))
        .or_else(|| {
            options
                .iter()
                .find(|o| o["optionId"].as_str() == Some("allow"))
        })
        .and_then(|o| o["optionId"].as_str())
        .unwrap_or("allow");
    let deny_option = kind("reject_once")
        .or_else(|| kind("reject_always"))
        .or_else(|| {
            options
                .iter()
                .find(|o| o["optionId"].as_str() == Some("deny"))
        })
        .and_then(|o| o["optionId"].as_str());

    let child_target = extract_child_target(v, spec.session_id);
    let (title, raw_detail, target) = permission_call(tool, spec.root);
    let detail = match child_target
        .as_ref()
        .and_then(|c| c.worker_label.as_deref())
    {
        Some(label) if !raw_detail.starts_with(&format!("[{label}]")) => {
            format!("[{label}] {}", one_line(&raw_detail))
        }
        _ => one_line(&raw_detail),
    };
    Parsed::Approval {
        title,
        detail,
        reply: json!({
            "id": v["id"],
            "allow_option": allow_option,
            "deny_option": deny_option,
            "child_session_id": child_target.map(|c| c.child_session_id),
        }),
        questions: Vec::new(),
        target,
    }
}

pub(super) fn parse_session_update(
    update: &Value,
    spec: &TurnSpec,
    message_id: &str,
    out: &mut Vec<Parsed>,
) {
    match update["sessionUpdate"].as_str() {
        Some("available_commands_update") => {
            let commands: Vec<ProviderSlashCommand> = update["availableCommands"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|cmd| {
                    let name = cmd["name"].as_str()?.trim();
                    if name.is_empty() {
                        return None;
                    }
                    Some(ProviderSlashCommand {
                        name: name.into(),
                        description: cmd["description"]
                            .as_str()
                            .map(str::trim)
                            .filter(|d| !d.is_empty())
                            .map(String::from),
                    })
                })
                .collect();
            if !commands.is_empty() {
                out.push(Parsed::SlashCommands(commands));
            }
        }
        Some("agent_message_chunk") => {
            if update["content"]["type"].as_str() == Some("text")
                && let Some(text) = update["content"]["text"].as_str().filter(|t| !t.is_empty())
            {
                out.push(
                    limit_notice(text)
                        .map_or_else(|| text_delta(message_id, text), |e| Parsed::Error(e.into())),
                );
            }
        }
        Some("agent_thought_chunk") => {
            if update["content"]["type"].as_str() == Some("text")
                && let Some(text) = update["content"]["text"].as_str().filter(|t| !t.is_empty())
            {
                out.push(thinking(message_id, text));
            }
        }
        Some("plan") => {
            let steps: Vec<PlanStep> = update["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|entry| {
                    let text = entry["content"].as_str()?.trim();
                    if text.is_empty() {
                        return None;
                    }
                    Some(PlanStep {
                        text: text.into(),
                        status: step_status(entry["status"].as_str()),
                    })
                })
                .collect();
            if !steps.is_empty() {
                out.push(Parsed::Event(AgentEvent::PlanUpdated {
                    message_id: message_id.into(),
                    steps,
                }));
            }
        }
        Some("tool_call" | "tool_call_update") => {
            let Some(id) = update["toolCallId"].as_str().filter(|s| !s.is_empty()) else {
                return;
            };
            let name = update["name"]
                .as_str()
                .or_else(|| update["kind"].as_str())
                .unwrap_or("tool");
            let args = tool_args(update);
            let start = if let Some((swarm_name, swarm_summary)) = format_swarm_tool(name, args) {
                ToolStart::new(id, swarm_name, swarm_summary)
            } else if name == "invoke_subagent"
                && let Some(subagent) = invoke_subagent_descriptor(args)
            {
                let summary = subagent.description.clone();
                ToolStart {
                    id,
                    name: "Agent",
                    summary,
                    subagent: Some(subagent),
                    parent: None,
                }
            } else {
                ToolStart::new(id, name, tool_summary(update, args, spec.root))
            };
            out.push(tool_started(message_id, start));

            let status = update["status"].as_str();
            if matches!(status, Some("completed" | "failed")) {
                let failed = status == Some("failed")
                    || update["rawOutput"]["exit_code"]
                        .as_i64()
                        .is_some_and(|c| c != 0)
                    || update["rawOutput"]["exitCode"]
                        .as_i64()
                        .is_some_and(|c| c != 0);
                if !failed && update["kind"].as_str() == Some("edit") {
                    for path in tool_paths(update, args) {
                        out.push(file_touched(message_id, relative(spec.root, &path)));
                    }
                }
                let output = tool_output(update);
                out.push(tool_finished(message_id, id, failed, &output));
            }
        }
        Some("usage_update") => {
            if let Some(used) = update["used"].as_u64().filter(|&n| n > 0) {
                out.push(Parsed::RequestUsage {
                    request_id: format!("acp-context-{message_id}"),
                    usage: TokenUsage {
                        context: used,
                        ..Default::default()
                    },
                    top_level: true,
                    model: None,
                });
            }
        }
        _ => {}
    }
}

fn tool_args(tool: &Value) -> &Value {
    if tool["rawInput"].is_object() {
        &tool["rawInput"]
    } else {
        &tool["args"]
    }
}

fn arg_str<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| args[*k].as_str())
        .filter(|s| !s.is_empty())
}

fn tool_summary(tool: &Value, args: &Value, root: &str) -> String {
    let path = arg_str(
        args,
        &[
            "Path",
            "path",
            "TargetFile",
            "file_path",
            "filePath",
            "AbsolutePath",
            "DirectoryPath",
            "SearchPath",
        ],
    )
    .or_else(|| tool["locations"].as_array()?.first()?.get("path")?.as_str());
    let command = arg_str(args, &["CommandLine", "command_line", "command", "cmd"]);
    match (path, command) {
        (Some(p), _) => relative(root, p),
        (None, Some(c)) => one_line(c),
        (None, None) => tool["title"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map_or_else(|| summarize(args, root), one_line),
    }
}

fn tool_paths(tool: &Value, args: &Value) -> Vec<String> {
    let mut paths: Vec<String> = tool["locations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|loc| loc["path"].as_str().map(String::from))
        .collect();
    if paths.is_empty()
        && let Some(p) = arg_str(
            args,
            &[
                "Path",
                "path",
                "TargetFile",
                "file_path",
                "filePath",
                "AbsolutePath",
            ],
        )
    {
        paths.push(p.to_string());
    }
    paths
}

fn tool_output(update: &Value) -> String {
    let blocks: Vec<&str> = update["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item["content"]["text"]
                .as_str()
                .or_else(|| item["text"].as_str())
        })
        .collect();
    if !blocks.is_empty() {
        return blocks.join("\n");
    }
    let raw = &update["rawOutput"];
    raw["output"]
        .as_str()
        .or_else(|| raw["combinedOutput"].as_str())
        .or_else(|| raw["combined_output"].as_str())
        .or_else(|| raw["error"].as_str())
        .or_else(|| raw.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Maps an ACP `session/request_permission` tool call to a Patchwork approval title, detail, and
/// permission rule target.
fn permission_call(tool: &Value, root: &str) -> (String, String, Option<Target>) {
    let name = tool["name"].as_str().unwrap_or_default();
    let kind = tool["kind"].as_str().unwrap_or_default();
    let args = tool_args(tool);

    if kind == "execute" || matches!(name, "run_command" | "execute_command" | "shell") {
        let command = arg_str(args, &["CommandLine", "command_line", "command", "cmd"])
            .or_else(|| tool["title"].as_str())
            .unwrap_or_default();
        let target = (!command.is_empty())
            .then(|| Target::new(PermissionKind::Command, [command.to_string()]));
        return ("Run command".into(), command.into(), target);
    }

    if kind == "edit"
        || matches!(
            name,
            "write_file"
                | "write_to_file"
                | "edit_file"
                | "replace_file_content"
                | "multi_replace_file_content"
                | "create_file"
                | "delete_file"
        )
    {
        let paths = tool_paths(tool, args);
        let detail = if paths.is_empty() {
            tool["title"]
                .as_str()
                .unwrap_or("Edit workspace files")
                .to_string()
        } else {
            paths
                .iter()
                .map(|p| relative(root, p))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let target = (!paths.is_empty()).then(|| Target::new(PermissionKind::Edit, paths));
        return ("Edit files".into(), detail, target);
    }

    if kind == "read"
        || matches!(
            name,
            "read_file" | "view_file" | "list_dir" | "grep_search" | "find_by_name"
        )
    {
        let paths = tool_paths(tool, args);
        if let Some(first) = paths.first() {
            let folder = if name.contains("file") {
                Path::new(first)
                    .parent()
                    .map_or_else(|| first.clone(), |p| p.to_string_lossy().into_owned())
            } else {
                first.clone()
            };
            return (
                "Read outside the project".into(),
                relative(root, first),
                Some(Target::new(PermissionKind::Folder, [folder])),
            );
        }
    }

    if (kind == "fetch"
        || matches!(
            name,
            "read_url" | "read_url_content" | "execute_url" | "fetch_url"
        ))
        && let Some(url) = arg_str(args, &["Url", "url", "URL"])
    {
        return (
            "Fetch a web page".into(),
            url.into(),
            Some(Target::new(PermissionKind::Web, [url.to_string()])),
        );
    }

    if let Some((server, tool_name)) = name.strip_prefix("mcp__").and_then(|s| s.split_once("__")) {
        return (
            format!("Use {tool_name} from {server}"),
            one_line(&args.to_string()),
            Some(Target::new(
                PermissionKind::Mcp,
                [format!("{server}/{tool_name}")],
            )),
        );
    }

    let title = if name.is_empty() {
        "Allow action".into()
    } else {
        format!("Run {name}")
    };
    let detail = tool_summary(tool, args, root);
    (title, detail, None)
}
