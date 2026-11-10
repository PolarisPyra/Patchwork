//! A goal while turns pursue it: how long the agent's input stays open while Codex works toward
//! one, and how the goal is settled once a turn ends.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::state::{agent_keeps, held_key, signature};
use super::stop::clear_command;
use crate::orchestration::AgentEvent;
use crate::orchestration::turn::process::close_stdin_unless_steered;
use crate::orchestration::turn::start::Turn;
use crate::provider::{self, AgentKind};
use crate::threads::{Goal, GoalStatus, MessageStatus};

/// How long Codex has to start its next turn toward an active goal after one ends. It starts
/// one within milliseconds, so a longer wait means the goal is not being pursued.
const CODEX_GOAL_GRACE: Duration = Duration::from_secs(30);

/// Ends the agent's input once its turn is over. While Codex pursues a goal, the input stays
/// open for the grace window: a turn started inside it continues the run, and otherwise the
/// input closes. Starting a turn advances `round`, which cancels a close still waiting.
pub(in crate::orchestration) fn end_input(turn: &Turn, goal_run: bool, round: &Arc<AtomicU64>) {
    if !goal_run {
        close_stdin_unless_steered(&turn.stdin, &turn.steers);
        return;
    }
    let armed = round.fetch_add(1, Ordering::SeqCst) + 1;
    let (round, stdin, steers) = (
        Arc::clone(round),
        Arc::clone(&turn.stdin),
        Arc::clone(&turn.steers),
    );
    std::thread::spawn(move || {
        std::thread::sleep(CODEX_GOAL_GRACE);
        if round.load(Ordering::SeqCst) == armed {
            close_stdin_unless_steered(&stdin, &steers);
        }
    });
}

/// Whether the agent pursues `goal` in a run: an active one, or for Claude a paused one, which
/// Claude takes up again with the next message.
pub(in crate::orchestration) fn pursued(agent: AgentKind, goal: &Goal) -> bool {
    goal.status == GoalStatus::Active
        || (agent == AgentKind::Claude && goal.status == GoalStatus::Paused)
}

/// Settles the goal once a turn ends, from how it ended. `checked` says Claude's evaluator
/// checked a reply of this run against the goal. Returns the message that has the agent drop
/// its goal in a turn of its own, when it would otherwise take the goal up again.
pub(in crate::orchestration) fn settle(
    turn: &Turn,
    status: MessageStatus,
    cancelled: bool,
    checked: bool,
) -> Option<&'static str> {
    // The stored goal is read again, since the user may have cleared it meanwhile.
    let thread = turn.server.store.thread(&turn.thread_id);
    let goal = thread.as_ref().and_then(|t| t.goal.clone());
    let pursued = |g: &&Goal| pursued(turn.agent, g);
    let mut next = goal.clone();
    // Claude ended its run while still holding the goal, which it would take up again.
    let mut claude_holds = false;
    if let Some(g) = goal.as_ref().filter(pursued) {
        let status = if turn.agent == AgentKind::Claude && status == MessageStatus::Complete {
            // Without a session there is no verdict to read, and nothing is left pursuing
            // the goal.
            let judged = turn.session_id.as_deref().map_or(GoalStatus::Paused, |id| {
                provider::claude::goal_outcome(id, &g.objective)
            });
            // The evaluator only lets a checked reply end the run once the goal is met. When
            // it records no verdict (it can give up after 30 seconds and let the reply stand),
            // Claude still holds the goal, so it is cleared there too.
            if judged == GoalStatus::Paused && checked {
                claude_holds = turn.session_id.is_some();
                if claude_holds
                    && let Err(e) = turn
                        .server
                        .store
                        .set_state(&held_key(&turn.thread_id), &signature(g))
                {
                    log::warn!(error:% = e; "failed to note a held goal: {{error}}");
                }
                GoalStatus::Complete
            } else {
                judged
            }
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
        && goal.as_ref().as_ref().is_some_and(pursued)
        && thread.as_ref().is_some_and(|t| t.archived_at.is_none());
    let done = next
        .as_ref()
        .is_some_and(|g| matches!(g.status, GoalStatus::Complete | GoalStatus::Blocked));
    // The agent is told to drop a goal it would otherwise take up again.
    let mut clear_in_agent = None;
    if user_stopped || done {
        let ended = next.as_ref().map_or(GoalStatus::Paused, |g| g.status);
        if turn.session_id.is_some() && (claude_holds || agent_keeps(turn.agent, ended)) {
            clear_in_agent = Some(clear_command(turn.agent));
        }
    }
    if next != goal {
        turn.publish(AgentEvent::GoalUpdated { goal: next });
    }
    clear_in_agent
}
