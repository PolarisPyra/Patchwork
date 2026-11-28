//! The task registry: every task and session Patchwork knows of, in every project, as one list.
//!
//! A task is a chat, a sub agent a chat started, a background command a chat left running, a
//! scheduled task, or, when asked for, an agent session saved on this computer that Patchwork
//! did not run. Each is told with the application it belongs to, which is its project, where it
//! stands, and the capabilities that act on it.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::orchestration::tasks::chats::text;
use super::types::Ctx;
use crate::Error;
use crate::import::Session;
use crate::orchestration::tasks::dispatch;
use crate::projects::Project;
use crate::scheduled::Task;
use crate::system::now_ms;
use crate::chats::{BackgroundTask, MessageStatus, Role, TaskStatus, Chat};

/// The most tasks one answer lists.
const LISTED: usize = 60;

/// The most sessions from outside Patchwork one answer lists, newest first.
const OUTSIDE: usize = 25;

/// The most characters of a task's recent output.
const OUTPUT: usize = 240;

/// The states a task is in, in the order they are listed.
const STATES: &[&str] = &[
    "running",
    "waiting",
    "failed",
    "terminated",
    "idle",
    "cancelled",
    "completed",
];

/// The application of a chat that is in no project.
const OWN: &str = "Patchwork";

/// One task, with what it is sorted and counted by.
struct Entry {
    application: String,
    state: &'static str,
    /// When it last did something, in epoch milliseconds.
    at: u64,
    told: Value,
}

/// How long `ms` milliseconds are, in words.
fn span(ms: u64) -> String {
    let minutes = ms / 60_000;
    match minutes {
        0 => format!("{} s", ms / 1000),
        1..=59 => format!("{minutes} min"),
        60..=1439 => format!("{} h {} min", minutes / 60, minutes % 60),
        _ => format!("{} d {} h", minutes / 1440, (minutes % 1440) / 60),
    }
}

/// The end of `text`, which holds the newest of it.
fn tail(text: &str) -> String {
    let text = text.trim();
    let count = text.chars().count();
    if count <= OUTPUT {
        return text.to_string();
    }
    let kept: String = text.chars().skip(count - OUTPUT).collect();
    format!("…{kept}")
}

/// Where the chat stands. It is running only while its agent's process is replying, so a chat
/// whose reply was cut off by a crash or a restart is told as terminated.
fn chat_state(ctx: &Ctx<'_>, chat: &Chat) -> &'static str {
    let runner = &ctx.core.runner;
    let live = runner.is_running(&chat.id) || runner.is_starting(&chat.id);
    if (live && chat.ask().is_some()) || ctx.core.limit_resume_at(&chat.id).is_some() {
        return "waiting";
    }
    if live {
        return "running";
    }
    let reply = chat
        .messages
        .iter()
        .rfind(|m| m.role == Role::Assistant && !m.queued);
    match reply.map(|m| m.status) {
        None => "idle",
        // Its agent is still up between turns, so the session is open.
        Some(MessageStatus::Complete) if runner.has_agent(&chat.id) => "idle",
        Some(MessageStatus::Complete) => "completed",
        Some(MessageStatus::Error) => "failed",
        Some(MessageStatus::Cancelled) => "cancelled",
        Some(MessageStatus::Interrupted | MessageStatus::Streaming) => "terminated",
    }
}

/// The capabilities that act on a chat in `state`.
fn chat_actions(state: &str, asking: bool, at_limit: bool) -> Vec<&'static str> {
    let mut actions = vec!["read_chat", "open_chat"];
    match state {
        "running" => actions.extend(["send_to_chat", "wait_for_chat", "stop_chat"]),
        "waiting" if asking => actions.extend(["answer_chat", "stop_chat"]),
        "waiting" => actions.push("stop_chat"),
        _ => actions.extend(["send_to_chat", "rename_chat", "archive_chat", "delete_chat"]),
    }
    if at_limit {
        actions.push("resume_chat_at_reset");
    }
    actions
}

/// A chat or a sub agent as a task.
fn chat_entry(ctx: &Ctx<'_>, chat: &Chat, projects: &[Project], chats: &[Chat]) -> Entry {
    let now = now_ms();
    let state = chat_state(ctx, chat);
    let project = projects
        .iter()
        .find(|p| Some(&p.id) == chat.project_id.as_ref());
    let application = project.map_or_else(|| OWN.to_string(), |p| p.name.clone());
    let asked = chat
        .messages
        .iter()
        .rfind(|m| m.role == Role::User && !m.queued);
    let reply = chat
        .messages
        .iter()
        .rfind(|m| m.role == Role::Assistant && !m.queued);
    let started = asked.map_or(chat.created_at, |m| m.created_at);
    let ended = reply.and_then(|m| m.finished_at);
    let at = chat
        .messages
        .last()
        .map_or(chat.created_at, |m| m.finished_at.unwrap_or(m.created_at));
    let ask = chat.ask();
    let reset = ctx.core.limit_reset(chat);
    let resumes = ctx.core.limit_resume_at(&chat.id);
    let mut told = json!({
        "application": application,
        "kind": if chat.parent.is_some() { "sub agent" } else { "chat" },
        "name": chat.title,
        "chat_id": chat.id,
        "agent": chat.agent,
        "model": chat.model,
        "state": state,
        "started_at_epoch_ms": started,
        "last_activity": format!("{} ago", span(now.saturating_sub(at))),
    });
    let running = matches!(state, "running" | "waiting");
    if running {
        told["running_for"] = json!(span(now.saturating_sub(started)));
        told["doing"] = json!(chat.doing());
    } else if let Some(ended) = ended {
        told["ran_for"] = json!(span(ended.saturating_sub(started)));
    }
    if let Some(project) = project {
        told["working_directory"] = json!(project.path);
    }
    if let Some(reply) = reply.filter(|m| !m.text.trim().is_empty()) {
        told["recent_output"] = json!(tail(&reply.text));
    }
    if let Some(error) = reply.and_then(|m| m.error.as_ref()) {
        told["error"] = json!(tail(error));
    }
    let used: u64 = chat
        .messages
        .iter()
        .filter_map(|m| m.usage.as_ref())
        .map(|u| u.input + u.cache_write + u.output)
        .sum();
    if used > 0 {
        told["tokens_used"] = json!(used);
    }
    if let Some(context) = reply.and_then(|m| m.usage.as_ref()).map(|u| u.context) {
        told["context_tokens"] = json!(context);
    }
    if let Some(ask) = &ask {
        told["pending_approval"] = json!({"title": ask.title, "detail": tail(&ask.detail)});
    }
    if let Some(at) = resumes {
        told["usage"] = json!(format!(
            "waiting for its agent's usage limit; continues in {}",
            span(at.saturating_sub(now))
        ));
    } else if let Some(at) = reset {
        told["usage"] = json!(format!(
            "stopped at its agent's usage limit, which resets in {}",
            span(at.saturating_sub(now))
        ));
    }
    if let Some(parent) = &chat.parent {
        // A sub agent is steered through the chat that started it.
        told["started_by_chat_id"] = json!(parent.chat_id);
        told["started_by"] = json!(
            chats
                .iter()
                .find(|c| c.id == parent.chat_id)
                .map(|c| c.title.as_str())
        );
        told["actions"] = json!([]);
    } else {
        told["actions"] = json!(chat_actions(
            state,
            ask.is_some(),
            reset.is_some() && resumes.is_none()
        ));
    }
    Entry {
        application,
        state,
        at,
        told,
    }
}

/// The background commands the chat's latest reply left, as tasks. One still marked running
/// is told as terminated when the chat's agent is gone, since it ran inside that process.
fn background_entries(ctx: &Ctx<'_>, chat: &Chat, application: &str) -> Vec<Entry> {
    let Some(reply) = chat.messages.iter().rfind(|m| !m.background.is_empty()) else {
        return Vec::new();
    };
    let up = ctx.core.runner.has_agent(&chat.id);
    let now = now_ms();
    reply
        .background
        .iter()
        .map(|task: &BackgroundTask| {
            let state = match task.status {
                TaskStatus::Running if up => "running",
                TaskStatus::Waiting if up => "waiting",
                TaskStatus::Running | TaskStatus::Waiting => "terminated",
                TaskStatus::Done => "completed",
                TaskStatus::Failed => "failed",
                TaskStatus::Stopped => "cancelled",
            };
            let mut told = json!({
                "application": application,
                "kind": "background task",
                "name": task.description,
                "what": task.kind,
                "state": state,
                "started_at_epoch_ms": task.started_at,
                "in_chat": chat.title,
                "chat_id": chat.id,
                "agent": chat.agent,
                "actions": ["read_chat", "open_chat"],
            });
            if state == "running" {
                told["running_for"] = json!(span(now.saturating_sub(task.started_at)));
            }
            if let Some(summary) = &task.summary {
                told["doing"] = json!(tail(summary));
            }
            Entry {
                application: application.to_string(),
                state,
                at: task.started_at,
                told,
            }
        })
        .collect()
}

/// A scheduled task as a task: waiting for its next run when it is on, and idle when off.
fn scheduled_entry(task: &Task, projects: &[Project]) -> Entry {
    let project = projects
        .iter()
        .find(|p| Some(&p.id) == task.project_id.as_ref());
    let application = project.map_or_else(|| OWN.to_string(), |p| p.name.clone());
    let state = if task.enabled { "waiting" } else { "idle" };
    let told = json!({
        "application": application,
        "kind": "scheduled task",
        "name": task.name,
        "id": task.id,
        "agent": task.agent,
        "model": task.model,
        "state": state,
        "schedule": crate::scheduled::describe_task(task),
        "last_run_at_epoch_ms": task.last_run_at,
        "next_run_at_epoch_ms": task.next_run_at.filter(|_| task.enabled),
        "last_run": task.recent_runs.first().map(|run| json!({
            "outcome": run.outcome,
            "chat_id": run.chat_id,
            "detail": run.detail,
        })),
        "working_directory": project.map(|p| p.path.as_str()),
        "actions": ["run_scheduled_task", "set_scheduled_task_enabled", "delete_scheduled_task"],
    });
    Entry {
        application,
        state,
        at: u64::try_from(task.last_run_at.unwrap_or(task.created_at)).unwrap_or_default(),
        told,
    }
}

/// An agent session Patchwork did not run, as a task. Patchwork cannot see its process, so it
/// is never told as running.
fn outside_entry(session: &Session, projects: &[Project]) -> Entry {
    let project = projects
        .iter()
        .filter(|p| Path::new(&session.cwd).starts_with(&p.path))
        .max_by_key(|p| p.path.len());
    let application = project.map_or_else(
        || {
            Path::new(&session.cwd)
                .file_name()
                .map_or_else(|| OWN.to_string(), |n| n.to_string_lossy().into_owned())
        },
        |p| p.name.clone(),
    );
    let at = session
        .messages
        .last()
        .map_or(session.created_at, |m| m.created_at);
    let told = json!({
        "application": application,
        "kind": "agent session outside Patchwork",
        "name": session.title,
        "session_id": session.session_id,
        "agent": session.agent,
        "model": session.model,
        "state": "idle",
        "tracked": false,
        "started_at_epoch_ms": session.created_at,
        "last_activity": format!("{} ago", span(now_ms().saturating_sub(at))),
        "working_directory": session.cwd,
        "recent_output": session
            .messages
            .iter()
            .rfind(|m| m.role == Role::Assistant)
            .map(|m| tail(&m.text)),
        "actions": ["import_chats"],
    });
    Entry {
        application,
        state: "idle",
        at,
        told,
    }
}

/// Lists the tasks and sessions of every application, or of the one asked for.
///
/// # Errors
///
/// Returns an error when no application or state is called what `args` names.
pub(super) fn list(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let core = ctx.core;
    let settings = core.settings();
    let projects = settings.projects.clone();
    let wanted = text(args, "application");
    // Patchwork itself is every application's host, so asking for it asks for them all.
    let only = if wanted.is_empty() || wanted.eq_ignore_ascii_case(OWN) {
        None
    } else {
        let id = dispatch::project(&settings, wanted)?;
        projects.iter().find(|p| Some(&p.id) == id.as_ref())
    };
    let state = text(args, "state").to_lowercase();
    if !state.is_empty() && !STATES.contains(&state.as_str()) {
        return Err(format!("The states are: {}", STATES.join(", ")).into());
    }
    let agent = text(args, "agent").to_lowercase();
    let chats: Vec<Chat> = core
        .store
        .chats()
        .into_iter()
        .filter(|t| t.archived_at.is_none() && !core.is_dispatcher(&t.id))
        .collect();
    let mut entries = Vec::new();
    for chat in &chats {
        let entry = chat_entry(ctx, chat, &projects, &chats);
        entries.extend(background_entries(ctx, chat, &entry.application));
        entries.push(entry);
    }
    entries.extend(
        core.scheduled_tasks()
            .iter()
            .map(|task| scheduled_entry(task, &projects)),
    );
    if args["outside"].as_bool().unwrap_or(false) {
        let mut sessions = core.outside_sessions();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.messages.last().map(|m| m.created_at)));
        entries.extend(
            sessions
                .iter()
                .take(OUTSIDE)
                .map(|session| outside_entry(session, &projects)),
        );
    }
    entries.retain(|e| {
        only.is_none_or(|p| e.application == p.name)
            && (state.is_empty() || e.state == state)
            && (agent.is_empty() || e.told["agent"].as_str() == Some(agent.as_str()))
    });
    let rank = |state: &str| STATES.iter().position(|s| *s == state);
    entries.sort_by(|a, b| {
        rank(a.state)
            .cmp(&rank(b.state))
            .then_with(|| b.at.cmp(&a.at))
    });
    let mut counts: BTreeMap<&str, Map<String, Value>> = BTreeMap::new();
    for entry in &entries {
        let count = counts
            .entry(entry.application.as_str())
            .or_default()
            .entry(entry.state)
            .or_insert(json!(0));
        *count = json!(count.as_u64().unwrap_or(0) + 1);
    }
    let applications: Vec<Value> = counts
        .into_iter()
        .map(|(application, states)| json!({"application": application, "tasks": states}))
        .collect();
    let more = entries.len().saturating_sub(LISTED);
    let tasks: Vec<&Value> = entries.iter().take(LISTED).map(|e| &e.told).collect();
    let mut told = json!({"applications": applications, "tasks": tasks});
    if more > 0 {
        told["not_listed"] = json!(format!(
            "{more} more; narrow it with application, state or agent"
        ));
    }
    Ok(told.to_string())
}
