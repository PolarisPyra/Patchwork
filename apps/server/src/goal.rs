//! A thread's `/goal`: how the agent's reports change it, and how stopping it works.

use std::sync::Arc;

use crate::provider::GoalUpdate;
use crate::types::*;
use crate::{Res, Server};

fn dismissal_key(thread_id: &str) -> String {
    format!("dismissed-goal:{thread_id}")
}

fn signature(goal: &Goal) -> String {
    format!("{}:{}", goal.started_at, goal.objective)
}

pub(crate) fn is_dismissed(
    store: &crate::persistence::Store,
    thread_id: &str,
    goal: &Goal,
) -> bool {
    let dismissed = store.state(&dismissal_key(thread_id));
    dismissed.as_deref() == Some(signature(goal).as_str())
}

pub(crate) fn clear_dismissal(store: &crate::persistence::Store, thread_id: &str) -> Res<()> {
    store.set_state(&dismissal_key(thread_id), "")
}

pub(crate) fn update_is_dismissed(
    store: &crate::persistence::Store,
    thread_id: &str,
    update: &GoalUpdate,
) -> bool {
    match update {
        GoalUpdate::Reported(goal) => is_dismissed(store, thread_id, goal),
        GoalUpdate::Set { objective, .. } => store
            .state(&dismissal_key(thread_id))
            .and_then(|value| value.split_once(':').map(|(_, text)| text == objective))
            .unwrap_or(false),
        _ => false,
    }
}

/// The thread's goal after the agent reports `update`.
pub(crate) fn next_goal(current: Option<&Goal>, update: GoalUpdate) -> Option<Goal> {
    match update {
        GoalUpdate::Set { objective, checks } => Some(Goal {
            // Restating the same goal keeps when it was set.
            started_at: current
                .filter(|g| g.objective == objective)
                .map_or_else(now_ms, |g| g.started_at),
            objective,
            status: GoalStatus::Active,
            checks,
            tokens_used: None,
            token_budget: None,
            time_used_seconds: None,
        }),
        GoalUpdate::Checked => current.map(|g| Goal {
            status: GoalStatus::Active,
            checks: Some(g.checks.unwrap_or(0) + 1),
            ..g.clone()
        }),
        GoalUpdate::Cleared => None,
        GoalUpdate::Reported(goal) => Some(goal),
    }
}

/// Whether the agent still holds a goal that ended in `status`, and so has to be told to drop
/// it: Claude restores an unmet goal whenever its session resumes, Codex keeps a stopped goal
/// paused, and Antigravity keeps a goal per conversation. Claude has already dropped a goal its
/// evaluator judged impossible (blocked).
pub(crate) fn agent_keeps(agent: AgentKind, status: GoalStatus) -> bool {
    status != GoalStatus::Complete && !(agent == AgentKind::Claude && status == GoalStatus::Blocked)
}

/// The message that has the agent drop its goal. Antigravity has no command to clear a goal;
/// its docs say to ask the agent.
pub(crate) fn clear_command(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Antigravity => "Stop the current goal.",
        _ => "/goal clear",
    }
}

impl Server {
    /// Clears the goal, in the agent's session too, so the agent does not pick it up again.
    /// A goal's final status remains in the thread until the user clears it. The agent's own
    /// instructions are still cleared when pursuit ends (see the end of `runner::drive`).
    pub fn stop_goal(self: &Arc<Self>, thread_id: &str) -> Res<()> {
        let Some(thread) = self.store.thread(thread_id) else {
            return Ok(());
        };
        // A `/goal` command runs as a turn of its own, so a running turn must be stopped first.
        if self.runner.is_running(thread_id) {
            return Err("Stop the running turn before clearing the goal".into());
        }
        let Some(goal) = thread.goal.as_ref() else {
            return Ok(());
        };
        self.store.dismiss_goal(thread_id, &signature(goal))?;
        self.emit(AgentEventEnvelope {
            thread_id: thread_id.into(),
            event: AgentEvent::GoalUpdated { goal: None },
        });
        // Explicitly closing the banner must clear the CLI's session state too, including a
        // goal marked complete: some CLIs can report that cached goal again on resume.
        if thread.session_id.is_some() {
            let stop = clear_command(thread.agent);
            self.send_message(thread_id.into(), stop.into(), vec![], false)?;
        }
        Ok(())
    }
}
