//! What the app keeps about a thread's goal: how the agent's reports change it, which goal the
//! user dismissed, and which one Claude still holds.

// Rust guideline compliant 2026-10-09

use crate::Error;
use crate::persistence::Store;
use crate::provider::{AgentKind, GoalUpdate};
use crate::system::now_ms;
use crate::threads::{Goal, GoalStatus};

fn dismissal_key(thread_id: &str) -> String {
    format!("dismissed-goal:{thread_id}")
}

pub(crate) fn signature(goal: &Goal) -> String {
    format!("{}:{}", goal.started_at, goal.objective)
}

pub(crate) fn is_dismissed(store: &Store, thread_id: &str, goal: &Goal) -> bool {
    let dismissed = store.state(&dismissal_key(thread_id));
    dismissed.as_deref() == Some(signature(goal).as_str())
}

pub(crate) fn clear_dismissal(store: &Store, thread_id: &str) -> Result<(), Error> {
    store.set_state(&dismissal_key(thread_id), "")
}

pub(crate) fn update_is_dismissed(store: &Store, thread_id: &str, update: &GoalUpdate) -> bool {
    match update {
        GoalUpdate::Reported(goal) => is_dismissed(store, thread_id, goal),
        GoalUpdate::Set { objective, .. } => store
            .state(&dismissal_key(thread_id))
            .and_then(|value| value.split_once(':').map(|(_, text)| text == objective))
            .unwrap_or(false),
        GoalUpdate::Checked | GoalUpdate::Checking | GoalUpdate::Cleared => false,
    }
}

pub(super) fn held_key(thread_id: &str) -> String {
    format!("held-goal:{thread_id}")
}

/// Whether `update` is Claude dropping a met goal it still held (see `settle`). The goal stays
/// shown as complete, and only this one clear is taken that way.
pub(crate) fn update_releases_held(
    store: &Store,
    thread_id: &str,
    current: Option<&Goal>,
    update: &GoalUpdate,
) -> bool {
    let held = store.state(&held_key(thread_id));
    let releases = matches!(update, GoalUpdate::Cleared)
        && current.is_some_and(|g| held.as_deref() == Some(signature(g).as_str()));
    if releases && let Err(e) = store.set_state(&held_key(thread_id), "") {
        log::warn!(error:% = e; "failed to forget a held goal: {{error}}");
    }
    releases
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
        GoalUpdate::Checking => current.cloned(),
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

fn cut_off_key(thread_id: &str) -> String {
    format!("cut-off-goal:{thread_id}")
}

/// Notes whether the thread's goal was being pursued when the last app session closed, which
/// paused it. Written at each launch for a thread with a goal and no turn waiting to be resumed,
/// so a note lasts only until its turn is resumed or the conversation moves on.
pub(crate) fn note_cut_off(store: &Store, thread_id: &str, goal: &Goal, cut_off: bool) {
    let note = if cut_off {
        signature(goal)
    } else {
        String::new()
    };
    if store.state(&cut_off_key(thread_id)).unwrap_or_default() != note
        && let Err(e) = store.set_state(&cut_off_key(thread_id), &note)
    {
        log::warn!(error:% = e; "failed to note a goal the app's closing cut off: {{error}}");
    }
}

/// Whether `goal` is the one the app's closing cut off, still paused by it, and forgets that:
/// the turn being resumed takes the goal up again, once.
pub(crate) fn take_cut_off(store: &Store, thread_id: &str, goal: &Goal) -> bool {
    let key = cut_off_key(thread_id);
    let cut_off = goal.status == GoalStatus::Paused
        && store.state(&key).as_deref() == Some(signature(goal).as_str());
    if cut_off && let Err(e) = store.set_state(&key, "") {
        log::warn!(error:% = e; "failed to forget a cut-off goal: {{error}}");
    }
    cut_off
}
