//! Reads what `codex app-server` sends back into agent events.

use serde_json::{Value, json};

use crate::provider::{GoalCommand, goal_command};
use crate::permissions::Target;
use crate::orchestration::AgentEvent;
use crate::threads::BackgroundTask;
use crate::threads::SubAgent;
use crate::threads::TaskStatus;
use crate::threads::ToolCall;
use crate::system::now_ms;
use crate::threads::TokenUsage;
use crate::threads::PlanStep;
use crate::permissions::PermissionKind;
use crate::threads::Question;
use crate::threads::QuestionOption;
use crate::threads::Refusal;
use crate::threads::StepStatus;
use crate::provider::file_touched;
use crate::provider::GoalUpdate;
use crate::provider::one_line;
use crate::provider::Parsed;
use crate::provider::relative;
use crate::provider::text_delta;
use crate::provider::thinking;
use crate::provider::tool_finished;
use crate::provider::tool_started;
use crate::provider::ToolStart;
use crate::provider::TurnSpec;

use super::{
    begin, goal_of, goal_reply, GOAL_REQUEST, GOAL_SETUP, INTERRUPT_REQUEST, SKILLS_REQUEST,
    STEER_REQUEST, thread_params, THREAD_REQUEST, TURN_REQUEST,
};

fn plan_step_status(status: Option<&str>) -> StepStatus {
    match status {
        Some("inProgress") => StepStatus::Running,
        Some("completed") => StepStatus::Done,
        _ => StepStatus::Pending,
    }
}

/// Maps `item/started` / `item/completed`.
fn codex_item(
    completed: bool,
    item: &Value,
    spec: &TurnSpec,
    message_id: &str,
    out: &mut Vec<Parsed>,
) {
    let root = spec.root;
    let id = item["id"].as_str().unwrap_or_default();
    let failed = matches!(item["status"].as_str(), Some("failed" | "declined"));
    match item["type"].as_str() {
        // A subagent starting or ending. Its own frames come under its thread, `agentThreadId`,
        // which is also the id of the call that shows it. Its path ends in the name the model
        // gave it; the agent it runs as is on its thread, so the call shows once that is read.
        Some("subAgentActivity") => {
            let Some(thread) = item["agentThreadId"].as_str() else {
                return;
            };
            match item["kind"].as_str() {
                Some("started") if !completed => {
                    let name = item["agentPath"].as_str().unwrap_or_default();
                    let name = name.rsplit('/').next().unwrap_or_default();
                    out.push(Parsed::Send(
                        json!({"id": format!("{SUBAGENT_READ} {thread} {name}"), "method": "thread/read",
                            "params": {"threadId": thread}})
                        .to_string(),
                    ));
                }
                Some("completed") if completed => {
                    out.push(tool_finished(message_id, thread, false, ""))
                }
                _ => {}
            }
        }
        Some("plan") if completed => {
            let text = item["text"].as_str().unwrap_or_default().trim();
            if !text.is_empty() {
                out.push(Parsed::Event(AgentEvent::PlanProposed {
                    message_id: message_id.into(),
                    text: text.into(),
                }));
            }
        }
        Some("agentMessage") if completed => {
            let text = item["text"].as_str().unwrap_or_default();
            if !text.is_empty() {
                out.push(text_delta(message_id, text));
            }
        }
        Some("commandExecution") => {
            let command = bare_command(item["command"].as_str().unwrap_or_default());
            out.push(tool_started(
                message_id,
                ToolStart {
                    id,
                    name: "Shell",
                    summary: one_line(&command),
                    subagent: None,
                    parent: None,
                },
            ));
            if completed {
                let exit_failed = item["exitCode"].as_i64().is_some_and(|c| c != 0);
                let output = item["aggregatedOutput"].as_str().unwrap_or_default();
                out.push(tool_finished(message_id, id, failed || exit_failed, output));
            }
        }
        Some("mcpToolCall") => {
            let summary = format!(
                "{}.{}",
                item["server"].as_str().unwrap_or_default(),
                item["tool"].as_str().unwrap_or_default()
            );
            out.push(tool_started(
                message_id,
                ToolStart {
                    id,
                    name: "MCP",
                    summary,
                    subagent: None,
                    parent: None,
                },
            ));
            if completed {
                out.push(tool_finished(message_id, id, failed, ""));
            }
        }
        Some("webSearch") => {
            let query = item["query"].as_str().unwrap_or_default();
            out.push(tool_started(
                message_id,
                ToolStart {
                    id,
                    name: "WebSearch",
                    summary: one_line(query),
                    subagent: None,
                    parent: None,
                },
            ));
            if completed {
                out.push(tool_finished(message_id, id, false, ""));
            }
        }
        Some("fileChange") => {
            let paths: Vec<String> = item["changes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c["path"].as_str())
                .map(|p| relative(root, p))
                .collect();
            out.push(tool_started(
                message_id,
                ToolStart {
                    id,
                    name: "Edit",
                    summary: one_line(&paths.join(", ")),
                    subagent: None,
                    parent: None,
                },
            ));
            if completed {
                if !failed {
                    out.extend(paths.into_iter().map(|p| file_touched(message_id, p)));
                }
                out.push(tool_finished(message_id, id, failed, ""));
            }
        }
        _ => {}
    }
}

/// The command the model wrote, without the `/bin/zsh -lc '…'` Codex wraps each one in to run it
/// in the login shell. Approvals already show it bare, so a command reads the same in both.
fn bare_command(command: &str) -> String {
    match shell_words(command).as_slice() {
        [shell, flag, inner]
            if matches!(
                shell.rsplit('/').next(),
                Some("sh" | "bash" | "zsh" | "dash")
            ) && matches!(flag.as_str(), "-c" | "-lc") =>
        {
            inner.clone()
        }
        _ => command.to_string(),
    }
}

/// `line` split into words with a POSIX shell's quoting removed.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                word.extend(chars.by_ref().take_while(|&c| c != '\''));
            }
            '"' => {
                in_word = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some(e @ ('"' | '\\' | '$' | '`')) => word.push(e),
                            Some(e) => word.extend(['\\', e]),
                            None => word.push('\\'),
                        },
                        c => word.push(c),
                    }
                }
            }
            '\\' => {
                in_word = true;
                word.extend(chars.next());
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    words
}

/// The start of the id of a `thread/read` for a subagent's thread, which goes on with the
/// thread's id and the name the model gave the subagent.
const SUBAGENT_READ: &str = "subagent";

/// The call that shows the subagent on `thread`, from the read of that thread, or `name` when
/// it could not be read.
fn subagent_started(
    thread: &str,
    name: &str,
    read: &Value,
    spec: &TurnSpec,
    message_id: &str,
) -> Parsed {
    let found = &read["result"]["thread"];
    let kind = subagent_kind(found["agentRole"].as_str().unwrap_or(name), spec);
    let subagent = SubAgent {
        kind: kind.clone(),
        description: kind.clone(),
        model: found["model"].as_str().map(String::from),
        prompt: None,
    };
    tool_started(
        message_id,
        ToolStart {
            id: thread,
            name: "Agent",
            summary: kind,
            subagent: Some(subagent),
            parent: None,
        },
    )
}

/// The custom agent `name` is, when it is one, else `name` as written. Codex can write a role's
/// `-` as `_`, so a custom agent is found by either.
fn subagent_kind(name: &str, spec: &TurnSpec) -> String {
    spec.agents
        .iter()
        .find(|a| a.name == name || a.name.replace('-', "_") == name)
        .map_or_else(|| name.replace('_', "-"), |a| a.name.clone())
}

/// What a subagent's own frames add to its thread: its text, and the calls it makes.
pub fn subagent_events(v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<AgentEvent> {
    let mut out = Vec::new();
    if let Some(kind @ ("item/started" | "item/completed")) = v["method"].as_str() {
        codex_item(
            kind == "item/completed",
            &v["params"]["item"],
            spec,
            message_id,
            &mut out,
        );
    }
    out.into_iter()
        .filter_map(|p| match p {
            Parsed::Event(event) => Some(event),
            _ => None,
        })
        .collect()
}

/// The background task an open Codex call becomes when its turn completes with the call still
/// running: a command, or a subagent thread. Codex has no request that stops one, so the task has
/// no id to stop it by and is listed without a stop button. Its only stop is the turn's own Stop,
/// which ends the process running it. Codex does not wake the agent when one ends either; the
/// agent sees the result only when it polls.
pub fn background_task(tool: &ToolCall) -> Option<BackgroundTask> {
    let kind = match tool.name.as_str() {
        "Shell" => "command",
        "Agent" if tool.subagent.is_some() => "agent",
        _ => return None,
    };
    Some(BackgroundTask {
        tool_id: tool.id.clone(),
        task_id: None,
        run_id: None,
        kind: kind.into(),
        description: tool.summary.clone(),
        status: TaskStatus::Running,
        started_at: tool.started_at.unwrap_or_else(now_ms),
        summary: None,
        output: None,
    })
}

/// Maps a request Codex sends us. Approvals wait for the user; anything else is refused.
fn codex_server_request(v: &Value) -> Parsed {
    let params = &v["params"];
    let reason = params["reason"].as_str().filter(|r| !r.is_empty());
    let (title, detail, target) = match v["method"].as_str() {
        Some("item/commandExecution/requestApproval") => (
            "Run command",
            params["command"].as_str().or(reason).unwrap_or_default(),
            Target::new(PermissionKind::Command, params["command"].as_str()),
        ),
        // The request names no files; a folder it wants to write in, at most.
        Some("item/fileChange/requestApproval") => (
            "Edit files",
            reason
                .or(params["grantRoot"].as_str())
                .unwrap_or("Apply the proposed file changes"),
            Target::new(PermissionKind::Edit, params["grantRoot"].as_str()),
        ),
        Some("item/tool/requestUserInput") if !codex_questions(&params["questions"]).is_empty() => {
            let questions = codex_questions(&params["questions"]);
            return Parsed::Approval {
                title: "Question".into(),
                detail: questions
                    .first()
                    .map(|q| q.question.clone())
                    .unwrap_or_default(),
                reply: json!({"id": v["id"], "question": true}),
                questions,
                target: None,
            };
        }
        _ => {
            return Parsed::Send(
                json!({"id": v["id"], "error": {"code": -32601, "message": "Not supported by Patchwork"}})
                    .to_string(),
            );
        }
    };
    Parsed::Approval {
        title: title.into(),
        detail: one_line(detail),
        reply: json!({"id": v["id"]}),
        questions: vec![],
        target: Some(target),
    }
}

/// The questions of a `request_user_input` call, each answered under its own id.
fn codex_questions(questions: &Value) -> Vec<Question> {
    let questions = questions.as_array().map_or(&[][..], Vec::as_slice);
    questions
        .iter()
        .filter_map(|q| {
            let options = q["options"].as_array().map_or(&[][..], Vec::as_slice);
            Some(Question {
                id: q["id"].as_str()?.into(),
                header: q["header"].as_str().unwrap_or_default().into(),
                question: q["question"].as_str()?.into(),
                options: options
                    .iter()
                    .filter_map(|o| {
                        Some(QuestionOption {
                            label: o["label"].as_str()?.into(),
                            description: o["description"].as_str().unwrap_or_default().into(),
                        })
                    })
                    .collect(),
                multi_select: false,
            })
        })
        .collect()
}

/// Maps one JSON-RPC message from `codex app-server`.
pub fn parse(v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
    let mut out = Vec::new();
    let params = &v["params"];
    match (v.get("id"), v["method"].as_str()) {
        // A response to one of our requests.
        (Some(id), None) => {
            let error = v["error"]["message"].as_str();
            let subagent = id
                .as_str()
                .and_then(|id| id.strip_prefix(SUBAGENT_READ)?.trim_start().split_once(' '));
            if let Some((thread, name)) = subagent {
                out.push(subagent_started(thread, name, v, spec, message_id));
            } else if id == SKILLS_REQUEST || id == INTERRUPT_REQUEST {
                // Nothing to do: the skills folder is only an addition, and Stop sends SIGTERM to
                // an agent that does not end its turn when interrupted.
            } else if id == STEER_REQUEST {
                out.push(match error {
                    Some(message) => Parsed::SteerRejected(message.into()),
                    None => Parsed::Steered,
                });
            } else if id == GOAL_SETUP {
                out.extend(
                    goal_of(&v["result"]["goal"]).map(|g| Parsed::Goal(GoalUpdate::Reported(g))),
                );
            } else if id == THREAD_REQUEST
                && spec.session_id.is_some()
                && error.is_some_and(|m| m.starts_with("no rollout found"))
            {
                // Codex saves a thread with its first turn, so one whose first turn failed cannot
                // be resumed. It holds nothing, so a new thread takes its place.
                out.push(Parsed::Send(
                    json!({"id": THREAD_REQUEST, "method": "thread/start", "params": thread_params(spec, None)})
                        .to_string(),
                ));
            } else if let Some(message) = error {
                out.extend([Parsed::Error(message.into()), Parsed::Finished]);
            } else if id == THREAD_REQUEST
                && let Some(thread_id) = v["result"]["thread"]["id"].as_str()
            {
                out.push(Parsed::SessionId(thread_id.into()));
                out.extend(begin(
                    spec,
                    thread_id,
                    v["result"]["thread"]["model"].as_str(),
                ));
            } else if id == TURN_REQUEST
                && matches!(
                    goal_command(spec.text),
                    Some(GoalCommand::Set(_) | GoalCommand::Resume)
                )
                && let Some(thread_id) = spec.session_id
            {
                // Its turn is running, so the goal can go active without Codex starting another.
                out.push(Parsed::Send(
                    json!({"id": GOAL_SETUP, "method": "thread/goal/set", "params": {
                        "threadId": thread_id, "status": "active",
                    }})
                    .to_string(),
                ));
            } else if id == GOAL_REQUEST {
                let result = &v["result"];
                if result.get("cleared").is_some() {
                    out.push(Parsed::Goal(GoalUpdate::Cleared));
                }
                out.extend(goal_of(&result["goal"]).map(|g| Parsed::Goal(GoalUpdate::Reported(g))));
                out.extend([
                    text_delta(message_id, &goal_reply(spec, result)),
                    Parsed::Finished,
                ]);
            }
        }
        (Some(_), Some(_)) => out.push(codex_server_request(v)),
        (None, Some("item/started")) if params["item"]["type"] == "contextCompaction" => {
            out.push(Parsed::Compacting);
        }
        (None, Some(kind @ ("item/started" | "item/completed"))) => {
            codex_item(
                kind == "item/completed",
                &params["item"],
                spec,
                message_id,
                &mut out,
            );
        }
        (None, Some("thread/goal/updated")) => {
            out.extend(goal_of(&params["goal"]).map(|g| Parsed::Goal(GoalUpdate::Reported(g))));
        }
        (None, Some("thread/goal/cleared")) => out.push(Parsed::Goal(GoalUpdate::Cleared)),
        (None, Some("item/reasoning/summaryTextDelta")) => {
            let delta = params["delta"].as_str().unwrap_or_default();
            if !delta.is_empty() {
                out.push(thinking(message_id, delta));
            }
        }
        (None, Some("turn/started")) => {
            if let Some(turn_id) = params["turn"]["id"].as_str() {
                out.push(Parsed::TurnStarted(turn_id.into()));
            }
        }
        (None, Some("turn/plan/updated")) => {
            let steps = params["plan"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|step| PlanStep {
                    text: step["step"].as_str().unwrap_or_default().into(),
                    status: plan_step_status(step["status"].as_str()),
                })
                .collect();
            out.push(Parsed::Event(AgentEvent::PlanUpdated {
                message_id: message_id.into(),
                steps,
            }));
        }
        (None, Some("error")) if params["willRetry"] != true => {
            if let Some(refusal) = policy_refusal(&params["error"]) {
                out.push(refused(message_id, refusal));
            } else {
                let message = params["error"]["message"]
                    .as_str()
                    .unwrap_or("Codex reported an error");
                out.push(Parsed::Error(message.into()));
            }
        }
        (None, Some("turn/completed")) => {
            let turn = &params["turn"];
            if turn["status"] == "failed" {
                if let Some(refusal) = policy_refusal(&turn["error"]) {
                    out.push(refused(message_id, refusal));
                } else {
                    let message = turn["error"]["message"].as_str().unwrap_or("Turn failed");
                    out.push(Parsed::Error(message.into()));
                }
            }
            out.push(Parsed::Finished);
        }
        // Sent after each model request; `last` is that request, and its total is the context.
        (None, Some("thread/tokenUsage/updated")) => {
            let usage = &params["tokenUsage"];
            let last = &usage["last"];
            let count = |key: &str| last[key].as_u64().unwrap_or(0);
            let cached = count("cachedInputTokens");
            out.push(Parsed::RequestUsage {
                request_id: format!(
                    "{}:{}",
                    params["turnId"].as_str().unwrap_or_default(),
                    usage["total"]["totalTokens"].as_u64().unwrap_or(0),
                ),
                usage: TokenUsage {
                    input: count("inputTokens").saturating_sub(cached),
                    cache_read: cached,
                    cache_write: 0,
                    output: count("outputTokens"),
                    context: count("totalTokens"),
                },
                top_level: true,
            });
        }
        _ => {}
    }
    out
}

/// A Codex error that means a safety check blocked the request, not that the turn broke.
fn policy_refusal(error: &Value) -> Option<Refusal> {
    let category = match error["codexErrorInfo"].as_str()? {
        "cyberPolicy" => "cyber",
        "misalignmentPolicyViolation" => "misalignment",
        _ => return None,
    };
    Some(Refusal {
        category: Some(category.into()),
        explanation: error["message"]
            .as_str()
            .unwrap_or("Codex blocked this request under its usage policy.")
            .into(),
    })
}

fn refused(message_id: &str, refusal: Refusal) -> Parsed {
    Parsed::Event(AgentEvent::Refused {
        message_id: message_id.into(),
        refusal,
    })
}
