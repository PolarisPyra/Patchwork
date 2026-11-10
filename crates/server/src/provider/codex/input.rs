//! What Patchwork sends `codex app-server`: its arguments, and the JSON-RPC requests that start, steer and stop a turn.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use std::collections::HashMap;

use serde_json::{Value, json};

use crate::provider::{GoalCommand, goal_command};
use crate::threads::AccessMode;
use crate::threads::Attachment;
use crate::threads::AttachmentKind;
use crate::threads::Goal;
use crate::provider::Parsed;
use crate::provider::prompt;
use crate::provider::prompt_text;
use crate::provider::TurnSpec;

use super::{configured_model, models};

/// `codex app-server` speaks JSON-RPC on stdio; `turn_input` starts the conversation.
///
/// Codex leaves out its `update_plan` tool unless asked, and that tool is what fills the plan above
/// the chat box. Its `request_user_input` tool, the question card, works only in plan mode unless
/// enabled.
pub fn turn_args() -> Vec<String> {
    [
        "app-server",
        "-c",
        "tools.update_plan.enabled=true",
        "-c",
        "features.default_mode_request_user_input=true",
    ]
    .map(String::from)
    .into()
}

/// A set compact window makes Codex compact once its context reaches it.
#[must_use]
pub fn compact_args(spec: &TurnSpec) -> Vec<String> {
    if spec.compact_window == 0 {
        return Vec::new();
    }
    vec![
        "-c".into(),
        format!("model_auto_compact_token_limit={}", spec.compact_window),
    ]
}

/// `-c` overrides that give Codex the custom agents as roles its `spawn_agent` starts by name.
/// A role's instructions and model live in a file of their own, which the role points to.
#[must_use]
pub fn agents_args(spec: &TurnSpec) -> Vec<String> {
    let models = models();
    let mut args = Vec::new();
    for agent in spec.agents {
        // JSON strings are TOML strings too.
        let mut role = format!("developer_instructions = {}\n", json!(agent.prompt.trim()));
        // Codex can't run another CLI's model, so the role takes the chat's.
        if let Some(model) = agent
            .model
            .as_deref()
            .and_then(|m| crate::agents::offered(m, models.iter().filter_map(|o| o.id.as_deref())))
        {
            let _ = writeln!(role, "model = {}", json!(model));
        }
        let file = match crate::agents::launch_file(
            "codex",
            spec.root,
            &format!("{}.toml", agent.name),
            &role,
        ) {
            Ok(file) => file,
            Err(e) => {
                log::warn!(
                    agent:% = agent.name, error:% = e;
                    "failed to hand Codex the {{agent}} agent: {{error}}"
                );
                continue;
            }
        };
        let key = format!("agents.{}", agent.name);
        args.extend([
            "-c".to_string(),
            format!("{key}.description={}", json!(agent.about())),
            "-c".to_string(),
            format!("{key}.config_file={}", json!(file.to_string_lossy())),
        ]);
    }
    args
}

/// Request id of `thread/start` / `thread/resume`; its response carries the thread id.
pub(super) const THREAD_REQUEST: u64 = 2;

/// Parameters of `thread/start`, or of `thread/resume` when `resume` names the thread.
pub(super) fn thread_params(spec: &TurnSpec, resume: Option<&str>) -> Value {
    let mut thread = json!({"cwd": spec.root});
    if let Some(m) = spec.model {
        thread["model"] = json!(m);
    }
    if let Some(text) = spec.instructions {
        thread["developerInstructions"] = json!(text);
    }
    if let Some(id) = resume {
        thread["threadId"] = json!(id);
        thread["excludeTurns"] = json!(true);
    }
    thread
}

/// Handshake, then start or resume the thread. `turn/start` follows once the thread id is known.
#[must_use]
pub fn turn_input(spec: &TurnSpec) -> Vec<String> {
    let thread = thread_params(spec, spec.session_id);
    let method = match spec.session_id {
        Some(_) => "thread/resume",
        None => "thread/start",
    };
    [
        json!({"id": 1, "method": "initialize", "params": {
            "clientInfo": {"name": "patchwork", "title": "Patchwork", "version": crate::VERSION},
            // turn/start collaborationMode is part of Codex's experimental app-server API.
            "capabilities": {"experimentalApi": true}
        }}),
        json!({"method": "initialized"}),
        // Patchwork's skills folder joins Codex's own skill folders.
        json!({"id": SKILLS_REQUEST, "method": "skills/extraRoots/set", "params": {
            "extraRoots": spec.skills_dir.into_iter().collect::<Vec<_>>(),
        }}),
        json!({"id": THREAD_REQUEST, "method": method, "params": thread}),
    ]
    .map(|v| v.to_string())
    .into()
}

/// Request id of `skills/extraRoots/set`. Its response, even an error from a Codex without the
/// method, does not end the turn.
pub(super) const SKILLS_REQUEST: u64 = 12;

/// How Codex is asked to present its plan in plan mode, so it arrives as a plan item.
const PLAN_INSTRUCTIONS: &str = "You are in plan mode: investigate and plan, but do not change \
files. When the plan is ready, present it once, wrapped in a <proposed_plan> block with the \
opening and closing tags each on their own line and Markdown inside. Give it a clear title, a \
short summary, the key changes, how to test them, and any assumptions. Do not ask whether to \
proceed; the user implements or refines the plan from there.";

/// Request id of `turn/start`.
pub(super) const TURN_REQUEST: u64 = THREAD_REQUEST + 1;

/// What the conversation's first request does once the thread id is known: start the
/// turn, or carry out a `/goal` command.
pub(super) fn begin(spec: &TurnSpec, thread_id: &str, resolved_model: Option<&str>) -> Vec<Parsed> {
    let send = |v: Value| Parsed::Send(v.to_string());
    let goal =
        |params: Value| json!({"id": GOAL_REQUEST, "method": "thread/goal/set", "params": params});
    match goal_command(spec.text) {
        // Codex compacts in a turn of its own, which it starts and reports like any other.
        None if spec.text.trim() == "/compact" && spec.attachments.is_empty() => vec![send(
            json!({"id": TURN_REQUEST, "method": "thread/compact/start", "params": {"threadId": thread_id}}),
        )],
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
            // The turn carries the attachments; the goal itself is the objective alone.
            Parsed::Send(turn_start(
                spec,
                thread_id,
                &prompt_text(objective, spec.attachments),
                resolved_model,
            )),
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
pub(super) const GOAL_REQUEST: u64 = 10;
/// Request id of the goal requests around a goal's turn; only their goal is kept.
pub(super) const GOAL_SETUP: u64 = 11;

#[must_use]
pub fn goal_pause(thread_id: &str) -> String {
    json!({"id": GOAL_SETUP, "method": "thread/goal/set", "params": {
        "threadId": thread_id, "status": "paused",
    }})
    .to_string()
}

/// The goal in a `thread/goal/*` response or notification.
pub(super) fn goal_of(v: &Value) -> Option<Goal> {
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

pub(super) fn goal_reply(spec: &TurnSpec, result: &Value) -> String {
    match goal_command(spec.text) {
        Some(GoalCommand::Clear) if result["cleared"] == true => "Goal cleared.".into(),
        Some(GoalCommand::Clear) => "No goal to clear.".into(),
        Some(GoalCommand::Pause) => "Goal paused. Send /goal resume to continue.".into(),
        Some(GoalCommand::Set(_) | GoalCommand::Resume | GoalCommand::Show) | None => {
            match goal_of(&result["goal"]) {
                Some(goal) => format!("Goal: {}", goal.objective),
                None => "No goal is set.".into(),
            }
        }
    }
}

/// The approval policy, reviewer, and sandbox for each access mode.
fn turn_start(
    spec: &TurnSpec,
    thread_id: &str,
    text: &str,
    resolved_model: Option<&str>,
) -> String {
    let (policy, reviewer, sandbox) = match spec.mode {
        AccessMode::Supervised => ("untrusted", "user", "readOnly"),
        AccessMode::AutoAcceptEdits => ("on-request", "user", "workspaceWrite"),
        AccessMode::Auto => ("on-request", "auto_review", "workspaceWrite"),
        AccessMode::FullAccess => ("never", "user", "dangerFullAccess"),
    };
    let mut params = json!({
        "threadId": thread_id,
        "cwd": spec.root,
        "input": if text.is_empty() { vec![] } else { input_items(text, spec.attachments, spec.skills_dir) },
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

fn input_items(text: &str, attachments: &[Attachment], skills_dir: Option<&str>) -> Vec<Value> {
    let mut input = vec![json!({"type": "text", "text": text})];
    // A message that invokes a skill names it as "$name", which Codex takes with the skill.
    if let Some(dir) = skills_dir {
        let skills = crate::skills::list(std::path::Path::new(dir));
        if let Some(skill) =
            crate::skills::mentioned(text).find_map(|name| skills.iter().find(|s| s.name == name))
        {
            input.push(json!({"type": "skill", "name": skill.name, "path": skill.path}));
        }
    }
    input.extend(
        attachments
            .iter()
            .filter(|a| a.kind == AttachmentKind::Image)
            .map(|a| json!({"type": "localImage", "path": a.path})),
    );
    input
}

/// Request id of `turn/steer`; its response is not a reason to end the turn.
pub(super) const STEER_REQUEST: u64 = 4;

/// `turn/steer` adds input to the turn that is running now, identified by its native id.
#[must_use]
pub fn steer_input(
    thread_id: &str,
    turn_id: &str,
    text: &str,
    attachments: &[Attachment],
) -> String {
    json!({"id": STEER_REQUEST, "method": "turn/steer", "params": {
        "threadId": thread_id,
        "expectedTurnId": turn_id,
        "input": input_items(text, attachments, None),
    }})
    .to_string()
}

pub(super) const INTERRUPT_REQUEST: u64 = 13;

/// `turn/interrupt` stops the turn running now; Codex ends it with `turn/completed` and the
/// status `interrupted`.
#[must_use]
pub fn interrupt(thread_id: &str, turn_id: &str) -> String {
    json!({"id": INTERRUPT_REQUEST, "method": "turn/interrupt", "params": {
        "threadId": thread_id,
        "turnId": turn_id,
    }})
    .to_string()
}

/// A question's reply carries the answers by question id; declining it answers none.
#[must_use]
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
