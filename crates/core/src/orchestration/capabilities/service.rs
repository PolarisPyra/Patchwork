//! The capabilities for the rest of Patchwork: its projects, usage limits, chat apps, models,
//! skills, scheduled tasks, and its own state and updates.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::integrations::reconnect;
use super::chats::text;
use super::types::Ctx;
use crate::Error;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::extensions::skills;
use crate::gateway::Platform;
use crate::notifications::resets_in;
use crate::orchestration::tasks::dispatch;
use crate::projects::Project;
use crate::provider::{AgentKind, list_models};
use crate::scheduled::Task;
use crate::system::{dirs, now_ms};
use crate::updates::{RestartResult, UpdateState};

/// How long a restart into an update waits for the reply that asked for it to end.
const REPLY_WAIT: Duration = Duration::from_secs(120);

/// How long the end of that reply is given to reach the chat app.
const REPLY_SENT: Duration = Duration::from_secs(3);

/// The most files `list_project_files` returns.
const FILES: usize = 200;

/// The agent called `name`.
pub(super) fn agent(name: &str) -> Result<AgentKind, Error> {
    serde_json::from_value::<AgentKind>(json!(name.to_lowercase())).map_err(|_unknown| {
        format!("Patchwork has no agent {name}. The agents are: claude, codex, antigravity, opencode, api").into()
    })
}

/// The project `args` names in the person's words.
pub(super) fn project(ctx: &Ctx<'_>, args: &Value) -> Result<Project, Error> {
    let settings = ctx.core.settings();
    let id = dispatch::project(&settings, text(args, "project"))?
        .ok_or("Give the project; list_projects names them")?;
    settings
        .projects
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| "That project is no longer here".into())
}

/// Tells the window that the settings changed, so it draws them.
pub(super) fn changed(ctx: &Ctx<'_>) {
    ctx.core.emit(AgentEventEnvelope {
        thread_id: String::new(),
        event: AgentEvent::SettingsChanged,
    });
}

/// Adds a folder on the computer as a project.
pub(super) fn add_project(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let path = text(args, "path");
    // A project's files can be listed, sent to the chat app and worked on by chats, so the
    // person agrees to every folder that becomes one.
    ctx.escalate("Patchwork: add a folder as a project", path)?;
    let added = ctx.core.add_project(path)?;
    changed(ctx);
    Ok(json!({"added": added.name, "folder": added.path}).to_string())
}

/// Renames a project.
pub(super) fn rename_project(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let found = project(ctx, args)?;
    let name = text(args, "name");
    ctx.core.rename_project(&found.id, name)?;
    changed(ctx);
    Ok(format!("Renamed {} to {name}.", found.name))
}

/// Removes a project and its chats from Patchwork. Its folder stays.
pub(super) fn remove_project(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let found = project(ctx, args)?;
    ctx.core.remove_project(&found.id)?;
    changed(ctx);
    Ok(format!(
        "Removed {} and its chats from Patchwork.",
        found.name
    ))
}

/// The files of a project, or the ones whose path holds some text.
pub(super) fn list_files(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let found = project(ctx, args)?;
    let holds = text(args, "contains").to_lowercase();
    let files: Vec<String> = ctx
        .core
        .list_files(&found.id)?
        .into_iter()
        .filter(|f| holds.is_empty() || f.to_lowercase().contains(&holds))
        .collect();
    Ok(json!({
        "project": found.name,
        "matching": files.len(),
        "files": files.iter().take(FILES).collect::<Vec<_>>(),
    })
    .to_string())
}

/// How much of each agent's usage limit is used and when it resets, as the provider reports it.
pub(super) fn usage(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let kinds = match text(args, "agent") {
        "" => ctx.core.settings().agents,
        name => vec![agent(name)?],
    };
    let now = now_ms();
    let limits: Vec<Value> = kinds
        .into_iter()
        .map(|kind| {
            let usage = ctx.core.usage.get(kind, true);
            let windows: Vec<Value> = usage
                .windows
                .iter()
                .map(|w| {
                    json!({
                        "limit": w.label,
                        "used_percent": w.used_percent,
                        // Only what the provider reported: a limit without a reset time has
                        // none to tell.
                        "resets_at_epoch_ms": w.resets_at,
                        "resets": w.resets_at.map(|at| resets_in(at, now)),
                    })
                })
                .collect();
            json!({
                "agent": kind,
                "plan": usage.plan,
                "limits": windows,
                "unavailable_because": usage.error,
                "reported_at_epoch_ms": usage.updated_at,
            })
        })
        .collect();
    Ok(json!({"now_epoch_ms": now, "usage": limits}).to_string())
}

/// Where each chat app's connection stands, who is paired, and what its chats start with.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every capability's handler has one signature"
)]
pub(super) fn gateway_status(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    let core = ctx.core;
    let settings = core.settings();
    let platforms: Vec<Value> = core
        .gateway_status()
        .into_iter()
        .map(|(platform, status)| {
            json!({"platform": platform.id(), "name": platform.label(), "status": format!("{status:?}")})
        })
        .collect();
    let paired: Vec<Value> = core
        .pairings()
        .into_iter()
        .map(|u| json!({"platform": u.platform.id(), "id": u.id, "name": u.name}))
        .collect();
    let usual = settings
        .gateway
        .project_id
        .as_ref()
        .and_then(|id| settings.projects.iter().find(|p| &p.id == id))
        .map(|p| p.name.clone());
    Ok(json!({
        "platforms": platforms,
        "paired": paired,
        "access": settings.gateway.access,
        "usual_project": usual,
        "personality": settings.gateway.personality,
    })
    .to_string())
}

/// Reconnects every chat app, as after a change to their settings.
pub(super) fn restart_gateway(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    reconnect(ctx)?;
    Ok("Reconnecting the chat apps. gateway_status shows where each one stands.".into())
}

/// Stops someone from using Patchwork from a chat app until they pair again.
pub(super) fn unpair(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let name = text(args, "platform").to_lowercase();
    let platform = Platform::ALL
        .into_iter()
        .find(|p| p.id() == name)
        .ok_or("No chat app is called that; gateway_status lists them")?;
    let id = text(args, "id");
    if !ctx
        .core
        .pairings()
        .iter()
        .any(|u| u.platform == platform && u.id == id)
    {
        return Err("No one with that id is paired there; gateway_status lists them".into());
    }
    ctx.core.remove_pairing(platform, id)?;
    Ok(format!(
        "{id} can no longer use Patchwork from {}.",
        platform.label()
    ))
}

/// The models an agent can run.
pub(super) fn models(_: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let models = list_models(agent(text(args, "agent"))?);
    serde_json::to_string(&models).map_err(|e| e.to_string().into())
}

/// The skills agents are given, and whether each is on.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every capability's handler has one signature"
)]
pub(super) fn skills(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    let listed: Vec<Value> = skills::all(&ctx.core.settings())
        .into_iter()
        .map(|s| json!({"name": s.name, "about": s.description, "on": s.enabled}))
        .collect();
    Ok(Value::Array(listed).to_string())
}

/// Turns a skill on or off for the agents started from now on.
pub(super) fn set_skill(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let name = text(args, "name");
    let on = args["on"].as_bool().unwrap_or(true);
    let mut settings = ctx.core.settings();
    if !skills::all(&settings).iter().any(|s| s.name == name) {
        return Err("No skill is called that; list_skills lists them".into());
    }
    settings.disabled_skills.retain(|s| s != name);
    if !on {
        settings.disabled_skills.push(name.to_string());
    }
    ctx.core.save_settings(settings)?;
    changed(ctx);
    Ok(format!("{name} is {} now.", if on { "on" } else { "off" }))
}

/// The scheduled task `args` names, by its id or its name.
fn task(ctx: &Ctx<'_>, args: &Value) -> Result<Task, Error> {
    let wanted = text(args, "task");
    let lower = wanted.to_lowercase();
    let tasks = ctx.core.scheduled_tasks();
    let mut found = tasks
        .iter()
        .filter(|t| t.id == wanted || t.name.to_lowercase().contains(&lower));
    match (found.next(), found.next()) {
        (Some(only), None) => Ok(only.clone()),
        (None, _) => Err("No scheduled task has that name; list_scheduled_tasks lists them".into()),
        _ => Err("Several scheduled tasks fit that name; pass the id of one".into()),
    }
}

/// The scheduled tasks, with when each runs next.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every capability's handler has one signature"
)]
pub(super) fn scheduled(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    let tasks: Vec<Value> = ctx
        .core
        .scheduled_tasks()
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "schedule": t.schedule,
                "agent": t.agent,
                "on": t.enabled,
                "last_run_at_epoch_ms": t.last_run_at,
                "next_run_at_epoch_ms": t.next_run_at,
            })
        })
        .collect();
    Ok(Value::Array(tasks).to_string())
}

/// Runs a scheduled task now, in a new chat.
pub(super) fn run_scheduled(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let found = task(ctx, args)?;
    let chat = ctx.core.run_scheduled_task(&found.id)?;
    Ok(json!({"started": found.name, "chat_id": chat.id, "title": chat.title}).to_string())
}

/// Turns a scheduled task on or off.
pub(super) fn set_scheduled(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let mut found = task(ctx, args)?;
    found.enabled = args["on"].as_bool().unwrap_or(true);
    ctx.core.save_scheduled_task(found.clone())?;
    Ok(format!(
        "{} is {} now.",
        found.name,
        if found.enabled { "on" } else { "off" }
    ))
}

/// Deletes a scheduled task.
pub(super) fn delete_scheduled(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let found = task(ctx, args)?;
    ctx.core.remove_scheduled_task(&found.id)?;
    Ok(format!("Deleted the scheduled task {}.", found.name))
}

/// Patchwork's version, whether an update waits, how many chats are replying, and where its
/// logs are.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every capability's handler has one signature"
)]
pub(super) fn diagnostics(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    let core = ctx.core;
    let chats = core.store.threads();
    let replying = chats
        .iter()
        .filter(|t| core.runner.is_running(&t.id) || core.runner.is_starting(&t.id))
        .count();
    let platforms: Vec<Value> = core
        .gateway_status()
        .into_iter()
        .map(|(platform, status)| json!({"platform": platform.id(), "status": format!("{status:?}")}))
        .collect();
    Ok(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "update": format!("{:?}", core.update_status().state),
        "chats": chats.len(),
        "replying": replying,
        "chat_apps": platforms,
        "logs_folder": dirs().logs,
    })
    .to_string())
}

/// Looks for a newer Patchwork.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every capability's handler has one signature"
)]
pub(super) fn check_for_updates(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    ctx.core.check_for_updates();
    Ok("Looking for an update. diagnostics shows what it found.".into())
}

/// Restarts Patchwork into an update that is downloaded and ready.
pub(super) fn install_update(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    if !matches!(ctx.core.update_status().state, UpdateState::Ready { .. }) {
        return Err("No update is downloaded and ready; diagnostics shows where it stands".into());
    }
    // The dispatcher's own reply is one of the chats replying, and it asked for this.
    let others = ctx.core.running_turns().saturating_sub(1);
    match ctx.core.restart_to_update(others == 0) {
        RestartResult::Restart => {
            ctx.core.note_update_asked(&ctx.caller.id)?;
            // The restart waits for this reply to be sent, so the person is not left without
            // an answer.
            let core = Arc::clone(ctx.core);
            let caller = ctx.caller.id.clone();
            std::thread::Builder::new()
                .name("update-restart".into())
                .spawn(move || {
                    let until = Instant::now() + REPLY_WAIT;
                    while core.runner.is_running(&caller) && Instant::now() < until {
                        std::thread::sleep(Duration::from_millis(250));
                    }
                    // The reply's last piece is on its way to the chat app.
                    std::thread::sleep(REPLY_SENT);
                    core.emit(AgentEventEnvelope {
                        thread_id: String::new(),
                        event: AgentEvent::RestartToUpdate,
                    });
                })
                .map_err(|e| format!("Patchwork could not start the restart: {e}"))?;
            Ok(
                "Patchwork restarts into the update as soon as this reply is sent. Tell the \
                 person it will be away for a moment, and that Patchwork says here when it is \
                 back and on which version. Do nothing else in this reply."
                    .into(),
            )
        }
        RestartResult::NothingReady => Err("No update is downloaded and ready".into()),
        RestartResult::Busy { .. } => Err(format!(
            "{others} other chats are still replying and Settings is set not to resume them, \
             so Patchwork did not restart. Stop them or wait, then ask again"
        )
        .into()),
    }
}
