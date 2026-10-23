//! A thread's `/goal`: how the agent's reports change it, how stopping it works, and how
//! long the agent's input stays open while Codex pursues one.

use super::*;
use crate::provider::GoalUpdate;

fn dismissal_key(thread_id: &str) -> String {
    format!("dismissed-goal:{thread_id}")
}

pub(crate) fn signature(goal: &Goal) -> String {
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
    /// instructions are still cleared when pursuit ends (see `settle`).
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

/// How long Codex has to start its next turn toward an active goal after one ends. It starts
/// one within milliseconds, so a longer wait means the goal is not being pursued.
pub(super) const CODEX_GOAL_GRACE: Duration = Duration::from_secs(30);

/// Ends the agent's input once its turn is over. While Codex pursues a goal, the input stays
/// open for the grace window: a turn started inside it continues the run, and otherwise the
/// input closes. Starting a turn advances `round`, which cancels a close still waiting.
pub(super) fn end_input(turn: &Turn, goal_run: bool, round: &Arc<AtomicU64>) {
    if !goal_run {
        close_stdin_unless_steered(&turn.stdin, &turn.steers);
        return;
    }
    let armed = round.fetch_add(1, Ordering::SeqCst) + 1;
    let (round, stdin, steers) = (round.clone(), turn.stdin.clone(), turn.steers.clone());
    std::thread::spawn(move || {
        std::thread::sleep(CODEX_GOAL_GRACE);
        if round.load(Ordering::SeqCst) == armed {
            close_stdin_unless_steered(&stdin, &steers);
        }
    });
}

/// Settles the goal once a turn ends, from how it ended. Returns the message that has the agent
/// drop its goal in a turn of its own, when it would otherwise take the goal up again.
pub(super) fn settle(turn: &Turn, status: MessageStatus, cancelled: bool) -> Option<&'static str> {
    // Claude's goal stands as its evaluator last judged it, which also covers a paused goal
    // that Claude took up again with this run's message. The stored goal is read again, since
    // the user may have cleared it meanwhile.
    let thread = turn.server.store.thread(&turn.thread_id);
    let goal = thread.as_ref().and_then(|t| t.goal.clone());
    let pursued = |g: &&Goal| {
        g.status == GoalStatus::Active
            || (turn.agent == AgentKind::Claude && g.status == GoalStatus::Paused)
    };
    let mut next = goal.clone();
    if let Some(g) = goal.as_ref().filter(pursued) {
        let status = if turn.agent == AgentKind::Claude && status == MessageStatus::Complete {
            // Without a session there is no verdict to read, and nothing is left pursuing
            // the goal.
            turn.session_id.as_deref().map_or(GoalStatus::Paused, |id| {
                provider::claude::goal_outcome(id, &g.objective)
            })
        } else if turn.agent == AgentKind::Antigravity && status == MessageStatus::Complete {
            // Antigravity blocks stopping until its goal is verified, so a clean end meets it.
            GoalStatus::Complete
        } else {
            // Failed or killed (a crash, the app quitting, a timeout): no agent is left
            // pursuing the goal, so it stays paused to resume.
            GoalStatus::Paused
        };
        next = Some(Goal {
            status,
            ..g.clone()
        });
    }
    // Keep the final goal state visible until the user closes it. Clear the agent's internal
    // goal instructions when pursuit ends, while retaining the app-side status for the banner.
    let user_stopped = cancelled
        && goal.as_ref().filter(pursued).is_some()
        && thread.as_ref().is_some_and(|t| t.archived_at.is_none());
    let done = next
        .as_ref()
        .is_some_and(|g| matches!(g.status, GoalStatus::Complete | GoalStatus::Blocked));
    // The agent is told to drop a goal it would otherwise take up again.
    let mut clear_in_agent = None;
    if user_stopped || done {
        let ended = next.as_ref().map_or(GoalStatus::Paused, |g| g.status);
        if turn.session_id.is_some() && goal::agent_keeps(turn.agent, ended) {
            clear_in_agent = Some(goal::clear_command(turn.agent));
        }
    }
    if next != goal {
        turn.publish(AgentEvent::GoalUpdated { goal: next });
    }
    clear_in_agent
}
