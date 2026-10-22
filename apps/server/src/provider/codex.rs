//! Codex (`codex app-server`, JSON-RPC over stdio).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::*;

/// `$CODEX_HOME`, else `~/.codex`.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".codex")))
}

pub fn auth(status: &mut AgentStatus) {
    let login = probe(AgentKind::Codex, &["login", "status"])
        .filter(|p| p.success)
        .and_then(|p| {
            p.stdout
                .lines()
                .chain(p.stderr.lines())
                .find(|l| l.contains("Logged in"))
                .map(|l| l.trim().to_string())
        });
    status.signed_in = login.is_some();
    status.detail = login.unwrap_or_else(|| "Not signed in — run `codex login`".into());
}

/// Read only model settings, without launching an inference request.
pub fn configured_model() -> Option<String> {
    let settings: toml::Value =
        toml::from_str(&std::fs::read_to_string(home()?.join("config.toml")).ok()?).ok()?;
    let profile_model = settings
        .get("profile")
        .and_then(toml::Value::as_str)
        .and_then(|profile| settings.get("profiles")?.get(profile)?.get("model"));
    Some(
        profile_model
            .or_else(|| settings.get("model"))?
            .as_str()?
            .to_owned(),
    )
}

/// Current Codex models, from the fetched model manifest.
pub fn models() -> Vec<ModelOption> {
    model_manifest::models("codex")
}

/// `(modified time, path)` of every rollout file under `dir`.
pub fn rollout_files(dir: &Path) -> Vec<(std::time::SystemTime, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "jsonl") {
                let modified = entry.metadata().and_then(|m| m.modified());
                out.push((modified.unwrap_or(std::time::UNIX_EPOCH), path));
            }
        }
    }
    out
}

/// `codex app-server` speaks JSON-RPC on stdio; `turn_input` starts the conversation.
pub fn turn_args() -> Vec<String> {
    vec!["app-server".into()]
}

/// Request id of `thread/start` / `thread/resume`; its response carries the thread id.
const THREAD_REQUEST: u64 = 2;

/// Parameters of `thread/start`, or of `thread/resume` when `resume` names the thread.
fn thread_params(spec: &TurnSpec, resume: Option<&str>) -> Value {
    let mut thread = json!({"cwd": spec.root});
    if let Some(m) = spec.model {
        thread["model"] = json!(m);
    }
    if let Some(id) = resume {
        thread["threadId"] = json!(id);
        thread["excludeTurns"] = json!(true);
    }
    thread
}

/// Handshake, then start or resume the thread. `turn/start` follows once the thread id is known.
pub fn turn_input(spec: &TurnSpec) -> Vec<String> {
    let thread = thread_params(spec, spec.session_id);
    let method = match spec.session_id {
        Some(_) => "thread/resume",
        None => "thread/start",
    };
    [
        json!({"id": 1, "method": "initialize", "params": {
            "clientInfo": {"name": "patchwork", "title": "Patchwork", "version": env!("CARGO_PKG_VERSION")},
            // turn/start collaborationMode is part of Codex's experimental app-server API.
            "capabilities": {"experimentalApi": true}
        }}),
        json!({"method": "initialized"}),
        json!({"id": THREAD_REQUEST, "method": method, "params": thread}),
    ]
    .map(|v| v.to_string())
    .into()
}

/// How Codex is asked to present its plan in plan mode, so it arrives as a plan item.
const PLAN_INSTRUCTIONS: &str = "You are in plan mode: investigate and plan, but do not change \
files. When the plan is ready, present it once, wrapped in a <proposed_plan> block with the \
opening and closing tags each on their own line and Markdown inside. Give it a clear title, a \
short summary, the key changes, how to test them, and any assumptions. Do not ask whether to \
proceed; the user implements or refines the plan from there.";

/// Request id of `turn/start`.
const TURN_REQUEST: u64 = THREAD_REQUEST + 1;

/// What the conversation's first request does once the thread id is known: start the
/// turn, or carry out a `/goal` command.
fn begin(spec: &TurnSpec, thread_id: &str, resolved_model: Option<&str>) -> Vec<Parsed> {
    let send = |v: Value| Parsed::Send(v.to_string());
    let goal =
        |params: Value| json!({"id": GOAL_REQUEST, "method": "thread/goal/set", "params": params});
    match goal_command(spec) {
        None => vec![Parsed::Send(turn_start(
            spec,
            thread_id,
            &prompt(spec),
            resolved_model,
        ))],
        // The goal waits paused until its first turn runs, so Codex does not start one first.
        Some(GoalCommand::Set(objective)) => vec![
            send(
                json!({"id": GOAL_SETUP, "method": "thread/goal/clear", "params": {"threadId": thread_id}}),
            ),
            send(
                json!({"id": GOAL_SETUP, "method": "thread/goal/set", "params": {
                    "threadId": thread_id, "objective": objective, "status": "paused",
                }}),
            ),
            Parsed::Send(turn_start(spec, thread_id, objective, resolved_model)),
        ],
        Some(GoalCommand::Resume) => vec![Parsed::Send(turn_start(
            spec,
            thread_id,
            "",
            resolved_model,
        ))],
        Some(GoalCommand::Show) => vec![send(
            json!({"id": GOAL_REQUEST, "method": "thread/goal/get", "params": {"threadId": thread_id}}),
        )],
        Some(GoalCommand::Clear) => vec![send(
            json!({"id": GOAL_REQUEST, "method": "thread/goal/clear", "params": {"threadId": thread_id}}),
        )],
        Some(GoalCommand::Pause) => vec![send(goal(
            json!({"threadId": thread_id, "status": "paused"}),
        ))],
    }
}

/// Request id of a `/goal` command's request; its response ends the turn.
const GOAL_REQUEST: u64 = 10;
/// Request id of the goal requests around a goal's turn; only their goal is kept.
const GOAL_SETUP: u64 = 11;

enum GoalCommand<'a> {
    Show,
    Clear,
    Pause,
    Resume,
    Set(&'a str),
}

/// `/goal` alone shows the goal; `clear` (or one of Claude's aliases for it), `pause`, and
/// `resume` act on it; anything else is a new objective.
fn goal_command<'a>(spec: &TurnSpec<'a>) -> Option<GoalCommand<'a>> {
    if !spec.attachments.is_empty() || !is_goal_command(spec.text) {
        return None;
    }
    let arg = spec.text.trim()["/goal".len()..].trim();
    Some(match arg.to_lowercase().as_str() {
        "" => GoalCommand::Show,
        "clear" | "stop" | "off" | "reset" | "none" | "cancel" => GoalCommand::Clear,
        "pause" => GoalCommand::Pause,
        "resume" => GoalCommand::Resume,
        _ => GoalCommand::Set(arg),
    })
}

pub fn goal_pause(thread_id: &str) -> String {
    json!({"id": GOAL_SETUP, "method": "thread/goal/set", "params": {
        "threadId": thread_id, "status": "paused",
    }})
    .to_string()
}

/// The goal in a `thread/goal/*` response or notification.
fn goal_of(v: &Value) -> Option<Goal> {
    let status = serde_json::from_value(v["status"].clone()).ok()?;
    // Codex counts seconds; older builds may count milliseconds.
    let created = v["createdAt"].as_u64().unwrap_or(0);
    Some(Goal {
        objective: v["objective"].as_str()?.into(),
        status,
        started_at: if created < 1_000_000_000_000 {
            created * 1000
        } else {
            created
        },
        checks: None,
        tokens_used: v["tokensUsed"].as_u64(),
        token_budget: v["tokenBudget"].as_u64(),
        time_used_seconds: v["timeUsedSeconds"].as_u64(),
    })
}

fn goal_reply(spec: &TurnSpec, result: &Value) -> String {
    match goal_command(spec) {
        Some(GoalCommand::Clear) if result["cleared"] == true => "Goal cleared.".into(),
        Some(GoalCommand::Clear) => "No goal to clear.".into(),
        Some(GoalCommand::Pause) => "Goal paused. Send /goal resume to continue.".into(),
        _ => match goal_of(&result["goal"]) {
            Some(goal) => format!("Goal: {}", goal.objective),
            None => "No goal is set.".into(),
        },
    }
}

/// The approval policy, reviewer, and sandbox for each access mode.
fn turn_start(spec: &TurnSpec, thread_id: &str, text: &str, resolved_model: Option<&str>) -> String {
    let (policy, reviewer, sandbox) = match spec.mode {
        AccessMode::Supervised => ("untrusted", "user", "readOnly"),
        AccessMode::AutoAcceptEdits => ("on-request", "user", "workspaceWrite"),
        AccessMode::Auto => ("on-request", "auto_review", "workspaceWrite"),
        AccessMode::FullAccess => ("never", "user", "dangerFullAccess"),
    };
    let mut params = json!({
        "threadId": thread_id,
        "cwd": spec.root,
        "input": if text.is_empty() { vec![] } else { input_items(text, spec.attachments) },
        "approvalPolicy": policy,
        "approvalsReviewer": reviewer,
        "sandboxPolicy": {"type": sandbox},
    });
    if let Some(m) = spec.model {
        params["model"] = json!(m);
    }
    if let Some(e) = spec.effort {
        params["effort"] = json!(e);
    }
    // Plan mode is a collaboration mode, which names its model; the turn leaves it again in
    // default mode. Without a model to name, the mode is not sent.
    if let Some(model) = spec
        .model
        .map(String::from)
        .or_else(|| resolved_model.map(String::from))
        .or_else(configured_model)
    {
        let (mode, instructions) = if spec.plan {
            ("plan", Some(PLAN_INSTRUCTIONS))
        } else {
            ("default", None)
        };
        params["collaborationMode"] = json!({"mode": mode, "settings": {
            "model": model,
            "reasoning_effort": spec.effort,
            "developer_instructions": instructions,
        }});
    }
    json!({"id": TURN_REQUEST, "method": "turn/start", "params": params}).to_string()
}

fn input_items(text: &str, attachments: &[Attachment]) -> Vec<Value> {
    let mut input = vec![json!({"type": "text", "text": text})];
    input.extend(
        attachments
            .iter()
            .filter(|a| a.kind == AttachmentKind::Image)
            .map(|a| json!({"type": "localImage", "path": a.path})),
    );
    input
}

/// Request id of `turn/steer`; its response is not a reason to end the turn.
const STEER_REQUEST: u64 = 4;

/// `turn/steer` adds input to the turn that is running now, identified by its native id.
pub fn steer_input(
    thread_id: &str,
    turn_id: &str,
    text: &str,
    attachments: &[Attachment],
) -> String {
    json!({"id": STEER_REQUEST, "method": "turn/steer", "params": {
        "threadId": thread_id,
        "expectedTurnId": turn_id,
        "input": input_items(text, attachments),
    }})
    .to_string()
}

/// A question's reply carries the answers by question id; declining it answers none.
pub fn approval_reply(
    reply: &Value,
    allow: bool,
    answers: &HashMap<String, Vec<String>>,
) -> String {
    if reply["question"] == true {
        let answers: serde_json::Map<_, _> = answers
            .iter()
            .filter(|_| allow)
            .map(|(id, picks)| (id.clone(), json!({"answers": picks})))
            .collect();
        return json!({"id": reply["id"], "result": {"answers": answers}}).to_string();
    }
    let decision = if allow { "accept" } else { "decline" };
    json!({"id": reply["id"], "result": {"decision": decision}}).to_string()
}

fn plan_step_status(status: Option<&str>) -> StepStatus {
    match status {
        Some("inProgress") => StepStatus::Running,
        Some("completed") => StepStatus::Done,
        _ => StepStatus::Pending,
    }
}

/// Maps `item/started` / `item/completed`.
fn codex_item(completed: bool, item: &Value, root: &str, message_id: &str, out: &mut Vec<Parsed>) {
    let id = item["id"].as_str().unwrap_or_default();
    let failed = matches!(item["status"].as_str(), Some("failed" | "declined"));
    match item["type"].as_str() {
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
            let command = item["command"].as_str().unwrap_or_default();
            out.push(tool_started(
                message_id,
                id,
                "Shell",
                one_line(command),
                None,
                None,
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
            out.push(tool_started(message_id, id, "MCP", summary, None, None));
            if completed {
                out.push(tool_finished(message_id, id, failed, ""));
            }
        }
        Some("webSearch") => {
            let query = item["query"].as_str().unwrap_or_default();
            out.push(tool_started(
                message_id,
                id,
                "WebSearch",
                one_line(query),
                None,
                None,
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
                id,
                "Edit",
                one_line(&paths.join(", ")),
                None,
                None,
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

/// Maps a request Codex sends us. Approvals wait for the user; anything else is refused.
fn codex_server_request(v: &Value) -> Parsed {
    let params = &v["params"];
    let reason = params["reason"].as_str().filter(|r| !r.is_empty());
    let (title, detail) = match v["method"].as_str() {
        Some("item/commandExecution/requestApproval") => (
            "Run command",
            params["command"].as_str().or(reason).unwrap_or_default(),
        ),
        Some("item/fileChange/requestApproval") => (
            "Edit files",
            reason
                .or(params["grantRoot"].as_str())
                .unwrap_or("Apply the proposed file changes"),
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
            if id == STEER_REQUEST {
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
                    goal_command(spec),
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
        (None, Some(kind @ ("item/started" | "item/completed"))) => {
            codex_item(
                kind == "item/completed",
                &params["item"],
                spec.root,
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
            let message = params["error"]["message"]
                .as_str()
                .unwrap_or("Codex reported an error");
            out.push(Parsed::Error(message.into()));
        }
        (None, Some("turn/completed")) => {
            let turn = &params["turn"];
            if turn["status"] == "failed" {
                let message = turn["error"]["message"].as_str().unwrap_or("Turn failed");
                out.push(Parsed::Error(message.into()));
            }
            out.push(Parsed::Finished);
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(session_id: Option<&'static str>, mode: AccessMode) -> TurnSpec<'static> {
        TurnSpec {
            root: "/abs",
            session_id,
            model: Some("gpt"),
            text: "go",
            mode,
            plan: false,
            effort: None,
            attachments: &[],
            images: &[],
        }
    }

    fn sent(out: &[Parsed]) -> Vec<Value> {
        out.iter()
            .filter_map(|p| match p {
                Parsed::Send(line) => serde_json::from_str(line).ok(),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn codex_starts_or_resumes_then_starts_the_turn() {
        let new: Vec<Value> = turn_input(&spec(None, AccessMode::FullAccess))
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(new[0]["method"], "initialize");
        assert_eq!(new[0]["params"]["capabilities"]["experimentalApi"], true);
        assert_eq!(new[1]["method"], "initialized");
        assert_eq!(new[2]["method"], "thread/start");
        assert_eq!(new[2]["params"]["cwd"], "/abs");

        let resumed = turn_input(&spec(Some("sid"), AccessMode::FullAccess));
        let resume: Value = serde_json::from_str(&resumed[2]).unwrap();
        assert_eq!(resume["method"], "thread/resume");
        assert_eq!(resume["params"]["threadId"], "sid");

        let response = json!({"id": THREAD_REQUEST, "result": {"thread": {
            "id": "t1", "model": "codex-resolved-model"
        }}});
        let mut plan_spec = spec(None, AccessMode::Supervised);
        plan_spec.model = None;
        plan_spec.plan = true;
        let out = parse(&response, &plan_spec, "m1");
        assert!(matches!(&out[0], Parsed::SessionId(id) if id == "t1"));
        let turn = &sent(&out)[0];
        assert_eq!(turn["method"], "turn/start");
        assert_eq!(turn["params"]["threadId"], "t1");
        assert_eq!(turn["params"]["input"][0]["text"], "go");
        assert_eq!(turn["params"]["approvalPolicy"], "untrusted");
        assert_eq!(turn["params"]["sandboxPolicy"]["type"], "readOnly");
        assert_eq!(turn["params"]["collaborationMode"]["mode"], "plan");
        assert_eq!(
            turn["params"]["collaborationMode"]["settings"]["model"],
            "codex-resolved-model"
        );
    }

    #[test]
    fn codex_message_and_command() {
        let spec = spec(None, AccessMode::FullAccess);
        let msg = json!({"method": "item/completed", "params": {"item":
            {"id": "i2", "type": "agentMessage", "text": "hi"}}});
        assert!(matches!(
            parse(&msg, &spec, "m1").as_slice(),
            [Parsed::Event(AgentEvent::TextDelta { text, .. })] if text == "hi"
        ));

        let started = json!({"method": "item/started", "params": {"item": {
            "id": "i3", "type": "commandExecution", "command": "cargo  test", "status": "inProgress"}}});
        assert!(matches!(
            parse(&started, &spec, "m1").as_slice(),
            [Parsed::Event(AgentEvent::ToolStarted { tool, .. })]
                if tool.name == "Shell" && tool.summary == "cargo test"
        ));

        let done = json!({"method": "item/completed", "params": {"item": {
            "id": "i3", "type": "commandExecution", "command": "cargo test",
            "exitCode": 1, "status": "failed"}}});
        assert!(matches!(
            parse(&done, &spec, "m1").last(),
            Some(Parsed::Event(AgentEvent::ToolFinished { tool_id, is_error: true, .. })) if tool_id == "i3"
        ));
    }

    #[test]
    fn codex_file_change_and_plan() {
        let spec = spec(None, AccessMode::FullAccess);
        let change = json!({"method": "item/completed", "params": {"item": {
            "id": "i4", "type": "fileChange", "status": "completed",
            "changes": [{"path": "/abs/src/a.ts", "kind": {"type": "update"}, "diff": ""}]}}});
        let out = parse(&change, &spec, "m1");
        assert!(out.iter().any(
            |p| matches!(p, Parsed::Event(AgentEvent::FileTouched { path, .. }) if path == "src/a.ts")
        ));

        let plan = json!({"method": "turn/plan/updated", "params": {"plan": [
            {"step": "a", "status": "completed"},
            {"step": "b", "status": "inProgress"},
            {"step": "c", "status": "pending"}]}});
        let out = parse(&plan, &spec, "m1");
        let [Parsed::Event(AgentEvent::PlanUpdated { steps, .. })] = out.as_slice() else {
            panic!("no plan");
        };
        let statuses: Vec<_> = steps.iter().map(|s| s.status).collect();
        assert_eq!(
            statuses,
            [StepStatus::Done, StepStatus::Running, StepStatus::Pending]
        );
    }

    #[test]
    fn codex_approvals_and_unsupported_requests() {
        let spec = spec(None, AccessMode::Supervised);
        let request = json!({"id": 7, "method": "item/commandExecution/requestApproval",
            "params": {"itemId": "i1", "threadId": "t", "turnId": "u", "startedAtMs": 0,
                "command": "rm -rf build"}});
        let out = parse(&request, &spec, "m1");
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
        let answer: Value =
            serde_json::from_str(&approval_reply(reply, false, &HashMap::new())).unwrap();
        assert_eq!(answer, json!({"id": 7, "result": {"decision": "decline"}}));

        let input = json!({"id": 8, "method": "item/tool/requestUserInput", "params": {}});
        assert_eq!(
            sent(&parse(&input, &spec, "m1"))[0]["error"]["code"],
            -32601
        );
    }

    #[test]
    fn codex_turn_end_and_errors() {
        let spec = spec(None, AccessMode::FullAccess);
        let retrying =
            json!({"method": "error", "params": {"error": {"message": "x"}, "willRetry": true}});
        assert!(parse(&retrying, &spec, "m1").is_empty());

        let failed = json!({"method": "turn/completed", "params": {"turn": {
            "status": "failed", "error": {"message": "usage limit"}}}});
        assert!(matches!(
            parse(&failed, &spec, "m1").as_slice(),
            [Parsed::Error(e), Parsed::Finished] if e == "usage limit"
        ));

        let rejected = json!({"id": 3, "error": {"code": -1, "message": "bad model"}});
        assert!(matches!(
            parse(&rejected, &spec, "m1").as_slice(),
            [Parsed::Error(e), Parsed::Finished] if e == "bad model"
        ));
    }
}
