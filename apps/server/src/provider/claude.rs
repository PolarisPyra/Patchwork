//! Claude Code (`claude -p` with stream-json input and output).

use std::path::PathBuf;

use serde_json::Value;

use super::*;

/// `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".claude")))
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

/// Current Claude models, from the fetched model manifest.
pub fn models() -> Vec<ModelOption> {
    model_manifest::models("claude")
}

/// The prompt is sent on stdin (`user_message`), so approvals can be answered there too.
pub fn turn_args(spec: &TurnSpec) -> Vec<String> {
    let mode = match spec.mode {
        AccessMode::Supervised => "default",
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

/// Asks Claude to stop one of its subagents.
pub fn stop_task(task_id: &str) -> String {
    serde_json::json!({
        "type": "control_request",
        "request_id": crate::types::new_id(),
        "request": {"subtype": "stop_task", "task_id": task_id},
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

fn claude_step_status(status: Option<&str>) -> StepStatus {
    match status {
        Some("in_progress") => StepStatus::Running,
        Some("completed") => StepStatus::Done,
        _ => StepStatus::Pending,
    }
}

/// Task calls carry a `subagent_type`; that is what starts a subagent.
fn subagent_of(input: &Value) -> Option<SubAgent> {
    Some(SubAgent {
        kind: input["subagent_type"].as_str()?.into(),
        description: input["description"].as_str().unwrap_or_default().into(),
        model: input["model"].as_str().map(String::from),
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
        id,
        name,
        summarize(input, root),
        subagent_of(input),
        parent,
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
                out.push(Parsed::TaskStarted {
                    tool_id: tool_id.into(),
                    task_id: task_id.into(),
                });
                // A command the agent backgrounded itself, not one inside a subagent.
                if v["task_type"] == "local_bash"
                    && v["is_backgrounded"] == true
                    && v["owned_by_subagent"] != true
                {
                    out.push(Parsed::BackgroundTask {
                        tool_id: tool_id.into(),
                        description: v["description"].as_str().unwrap_or_default().into(),
                    });
                }
            }
        }
        Some("rate_limit_event") => {
            let windows = crate::usage::claude::windows_from_event(&v["rate_limit_info"]);
            if !windows.is_empty() {
                out.push(Parsed::Usage(windows));
            }
        }
        Some("system") if v["subtype"] == "task_notification" => {
            if let Some(tool_id) = v["tool_use_id"].as_str() {
                out.push(Parsed::TaskEnded {
                    tool_id: tool_id.into(),
                    failed: v["status"] != "completed",
                });
            }
        }
        Some("system") if v["subtype"] == "init" => {
            if let Some(id) = v["session_id"].as_str() {
                out.push(Parsed::SessionId(id.into()));
            }
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
        }
        // Command output such as `/goal` comes as a whole synthetic message, not streamed.
        Some("assistant") if parent.is_none() && v["message"]["model"] == "<synthetic>" => {
            let text = block_text(blocks);
            if !text.is_empty() {
                out.push(text_delta(message_id, &text));
            }
            out.extend(goal_from_command(&text).map(Parsed::Goal));
        }
        Some("assistant") => {
            for block in blocks {
                if block["type"] == "tool_use" {
                    claude_tool_use(block, root, message_id, parent, &mut out);
                }
            }
            out.extend(request_usage(&v["message"], parent.is_none()));
        }
        Some("user") => {
            for block in blocks.iter().filter(|b| b["type"] == "tool_result") {
                let id = block["tool_use_id"].as_str().unwrap_or_default();
                let is_error = block["is_error"].as_bool().unwrap_or(false);
                // Content is a string, or text blocks.
                let content = &block["content"];
                let output = content.as_str().map_or_else(
                    || block_text(content.as_array().map_or(&[][..], Vec::as_slice)),
                    String::from,
                );
                out.push(tool_finished(message_id, id, is_error, &output));
            }
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
            if (is_error || subtype != "success") && !steered {
                let text = v["result"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(subtype);
                out.push(Parsed::Error(text.into()));
            }
            out.push(Parsed::Finished);
        }
        Some("control_request") => out.push(claude_control_request(v, root)),
        _ => {}
    }
    out
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
            objective: objective.trim().into(),
            checks: Some(0),
        });
    }
    if text.starts_with("Goal cleared") || text.starts_with("No goal set") {
        return Some(GoalUpdate::Cleared);
    }
    let rest = text.strip_prefix("Goal active: ")?;
    let line = rest.lines().next().unwrap_or(rest);
    let (objective, count) = line.rsplit_once(" (")?;
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
        .find(|a| a["type"] == "goal_status" && a["condition"] == objective);
    match verdict {
        Some(a) if a["met"] == true => GoalStatus::Complete,
        Some(a) if a["failed"] == true => GoalStatus::Blocked,
        // Unmet or not yet judged: the goal is still set in the session and goes on
        // with the next message.
        _ => GoalStatus::Paused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_init_is_session_and_noise_is_ignored() {
        let init =
            json!({"type": "system", "subtype": "init", "cwd": "/abs", "session_id": "9578"});
        let out = parse(&init, "/abs", "m1");
        assert!(matches!(out.as_slice(), [Parsed::SessionId(id)] if id == "9578"));

        let hook = json!({"type": "system", "subtype": "hook_started"});
        assert!(parse(&hook, "/abs", "m1").is_empty());
        let rate = json!({"type": "rate_limit_event"});
        assert!(parse(&rate, "/abs", "m1").is_empty());
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
            text: "do it",
            mode: AccessMode::FullAccess,
            effort: None,
            attachments: &[],
            images: &[],
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
                .any(|w| w == ["--permission-mode", "default"])
        );
        assert!(
            args.windows(2)
                .any(|w| w == ["--permission-prompt-tool", "stdio"])
        );
    }
}
