//! What a new app session does with the goals and replies the last one left behind: goals are
//! dismissed or paused, and what a cut-off reply left running is ended, ready to be resumed:
//! the tasks and calls it listed, and the agent process itself when the app was killed.

// Rust guideline compliant 2026-10-09

use crate::Error;
use crate::persistence::Store;
use crate::threads::CheckStatus;
use crate::threads::GoalStatus;
use crate::threads::MessageStatus;
use crate::threads::Role;
use crate::threads::StepStatus;
use crate::threads::TaskStatus;
use crate::system::now_ms;
use crate::system::{processes, scope};
use crate::workspace;
use crate::orchestration::goal;

/// The kinds of background task that are subagents: Claude's, and Codex's.
const SUBAGENT_TASKS: [&str; 2] = ["local_agent", "agent"];

/// Finished goals remain visible for the session in which they ended. A new app session
/// dismisses them, including cached reports from the provider on its next run. Unfinished
/// goals survive, paused if their agent was interrupted by quitting. A reply the last session
/// cut off stays interrupted, for `resume_interrupted` to carry on, with the tasks and calls it
/// left running ended; a subagent's cut-off reply ends as stopped.
pub(crate) fn recover(store: &Store) -> Result<(), Error> {
    for mut thread in store.threads() {
        end_left_agent(store, &thread.id);
        if let Some(finished) = thread
            .goal
            .as_ref()
            .filter(|goal| matches!(goal.status, GoalStatus::Complete | GoalStatus::Blocked))
        {
            store.dismiss_goal(&thread.id, &goal::signature(finished))?;
            thread.goal = None;
        }
        let dismissed = thread
            .goal
            .as_ref()
            .is_some_and(|g| goal::is_dismissed(store, &thread.id, g));
        // A goal the last session was pursuing goes on when its turn is resumed.
        // The note is kept while a turn still waits to be resumed, however many launches pass.
        if let Some(g) = thread.goal.as_ref().filter(|_| !dismissed) {
            let cut_off = store.goals_cut_off().contains(&thread.id);
            if cut_off || thread.interrupted_reply().is_none() {
                goal::note_cut_off(store, &thread.id, g, cut_off);
            }
        }
        let goal_active = thread
            .goal
            .as_ref()
            .is_some_and(|g| g.status == GoalStatus::Active);
        // A subagent's thread ends with the turn that ran it: resuming that turn starts its own.
        let child_cut_off = thread.parent.is_some()
            && thread
                .messages
                .iter()
                .any(|m| m.status == MessageStatus::Interrupted);
        // A reply that ended while its subagents still worked was not the end of its turn: the
        // agent was waiting to hear from them, so the turn was cut off as much as a reply in
        // flight is.
        let waiting = thread
            .messages
            .iter()
            .rfind(|m| !m.queued)
            .filter(|_| thread.parent.is_none())
            .filter(|m| m.role == Role::Assistant && m.status == MessageStatus::Complete)
            .filter(|m| {
                m.background
                    .iter()
                    .any(|task| task.status.is_live() && SUBAGENT_TASKS.contains(&&*task.kind))
            })
            .map(|m| m.id.clone());
        // A check that was running when the app closed never finished, so it is not a pass.
        let checking = thread.messages.iter().any(|m| {
            m.checks
                .as_ref()
                .is_some_and(|c| c.status == CheckStatus::Running)
        });
        // A task or a call still running when the app closed ended with the agent's process.
        let running = thread.messages.iter().any(|m| {
            m.background.iter().any(|task| task.status.is_live())
                || m.tools.iter().any(|t| t.status == StepStatus::Running)
        });
        if dismissed || goal_active || child_cut_off || checking || running {
            store.update_thread(&thread.id, |t| {
                if dismissed {
                    t.goal = None;
                } else if let Some(goal) =
                    t.goal.as_mut().filter(|g| g.status == GoalStatus::Active)
                {
                    goal.status = GoalStatus::Paused;
                }
                for message in &mut t.messages {
                    if child_cut_off && message.status == MessageStatus::Interrupted {
                        message.status = MessageStatus::Cancelled;
                    }
                    if waiting.as_ref() == Some(&message.id) {
                        message.status = MessageStatus::Interrupted;
                    }
                    for task in &mut message.background {
                        if task.status.is_live() {
                            task.status = TaskStatus::Stopped;
                        }
                    }
                    for tool in &mut message.tools {
                        if tool.status == StepStatus::Running {
                            tool.status = StepStatus::Error;
                            tool.ended_at = Some(now_ms());
                        }
                    }
                    if let Some(checks) = message
                        .checks
                        .as_mut()
                        .filter(|c| c.status == CheckStatus::Running)
                    {
                        checks.status = CheckStatus::NotRun;
                        checks.note =
                            Some("Patchwork closed while the checks were running.".into());
                    }
                }
            })?;
        }
    }
    Ok(())
}

/// The `app_state` key under which a thread's running agent process is noted.
fn agent_key(thread_id: &str) -> String {
    format!("agent-process:{thread_id}")
}

/// Notes the agent process a thread's turn runs in: its pid, when it started, and its scope.
///
/// A Patchwork that is killed cannot stop its agents, and one keeps working until its next write
/// fails. The note lets the next launch end it first, so a resumed turn never runs beside it.
/// Nothing is noted where the process table cannot be read, since a pid alone may be reused.
pub(in crate::orchestration) fn note_agent(
    store: &Store,
    thread_id: &str,
    pid: u32,
    scope: Option<&str>,
) {
    let Some(process) = i32::try_from(pid).ok().and_then(processes::find) else {
        return;
    };
    let note = format!("{pid}\n{}\n{}", process.started, scope.unwrap_or_default());
    if let Err(e) = store.set_state(&agent_key(thread_id), &note) {
        log::warn!(error:% = e; "could not note a turn's agent process: {{error}}");
    }
}

/// Drops the note for the agent process `pid`, which has exited. A note a newer turn of the
/// thread has since written is left.
pub(in crate::orchestration) fn forget_agent(store: &Store, thread_id: &str, pid: u32) {
    let key = agent_key(thread_id);
    let noted = store.state(&key);
    if noted.is_some_and(|note| note.lines().next() == Some(&*pid.to_string()))
        && let Err(e) = store.clear_state(&key)
    {
        log::warn!(error:% = e; "could not drop a turn's agent process note: {{error}}");
    }
}

/// Ends the agent the last session left running for the thread, if it still runs, with
/// everything it started. Only a process that is still the one noted is ended: same pid, same
/// start time.
fn end_left_agent(store: &Store, thread_id: &str) {
    let key = agent_key(thread_id);
    let Some(note) = store.state(&key) else {
        return;
    };
    let mut lines = note.lines();
    let pid = lines.next().and_then(|pid| pid.parse::<u32>().ok());
    let started = lines.next();
    let scope = lines.next().filter(|unit| !unit.is_empty());
    if let Some(pid) = pid.filter(|pid| *pid > 1 && *pid != std::process::id())
        && let Some(process) = i32::try_from(pid).ok().and_then(processes::find)
        && Some(process.started.as_str()) == started
    {
        log::info!(
            pid, thread = thread_id;
            "ending agent {{pid}}, left running for {{thread}} when Patchwork was killed"
        );
        workspace::signal_process_group(pid, workspace::SIGKILL);
        if let Some(unit) = scope {
            scope::stop(unit);
        }
    }
    if let Err(e) = store.clear_state(&key) {
        log::warn!(error:% = e; "could not drop a turn's agent process note: {{error}}");
    }
}
