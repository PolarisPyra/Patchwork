//! What the app keeps about a chat's goal: a new one, which goal the user dismissed, whether
//! its last reply went nowhere, and which one the app's closing cut off.

// Rust guideline compliant 2026-10-09

use crate::Error;
use crate::persistence::Store;
use crate::system::now_ms;
use crate::chats::{Goal, GoalStatus};

fn dismissal_key(chat_id: &str) -> String {
    format!("dismissed-goal:{chat_id}")
}

pub(crate) fn signature(goal: &Goal) -> String {
    format!("{}:{}", goal.started_at, goal.objective)
}

pub(crate) fn is_dismissed(store: &Store, chat_id: &str, goal: &Goal) -> bool {
    let dismissed = store.state(&dismissal_key(chat_id));
    dismissed.as_deref() == Some(signature(goal).as_str())
}

pub(crate) fn clear_dismissal(store: &Store, chat_id: &str) -> Result<(), Error> {
    store.set_state(&dismissal_key(chat_id), "")
}

/// The goal `/goal <objective>` sets. Restating the current goal keeps when it was set and the
/// time spent on it.
pub(crate) fn new_goal(current: Option<&Goal>, objective: &str) -> Goal {
    let now = now_ms();
    match current.filter(|g| g.objective == objective) {
        Some(goal) => Goal {
            checks: Some(0),
            ..goal.clone().with_status(GoalStatus::Active, now)
        },
        None => Goal {
            started_at: now,
            objective: objective.to_string(),
            status: GoalStatus::Active,
            checks: Some(0),
            time_used_seconds: None,
            active_since: None,
        },
    }
}

fn stall_key(chat_id: &str) -> String {
    format!("stalled-goal:{chat_id}")
}

/// Whether `goal` is going nowhere: this reply, which used no tools when `idle`, is the second
/// such reply in a row. A reply that used tools starts the count again.
pub(crate) fn stalled(store: &Store, chat_id: &str, goal: &Goal, idle: bool) -> bool {
    let key = stall_key(chat_id);
    let mark = signature(goal);
    let before = store.state(&key).as_deref() == Some(mark.as_str());
    let note = if idle && !before { mark } else { String::new() };
    if store.state(&key).unwrap_or_default() != note
        && let Err(e) = store.set_state(&key, &note)
    {
        log::warn!(error:% = e; "failed to note a goal reply that used no tools: {{error}}");
    }
    idle && before
}

fn cut_off_key(chat_id: &str) -> String {
    format!("cut-off-goal:{chat_id}")
}

/// Notes whether the chat's goal was being pursued when the last app session closed, which
/// paused it. Written at each launch for a chat with a goal and no turn waiting to be resumed,
/// so a note lasts only until its turn is resumed or the conversation moves on.
pub(crate) fn note_cut_off(store: &Store, chat_id: &str, goal: &Goal, cut_off: bool) {
    let note = if cut_off {
        signature(goal)
    } else {
        String::new()
    };
    if store.state(&cut_off_key(chat_id)).unwrap_or_default() != note
        && let Err(e) = store.set_state(&cut_off_key(chat_id), &note)
    {
        log::warn!(error:% = e; "failed to note a goal the app's closing cut off: {{error}}");
    }
}

/// Whether `goal` is the one the app's closing cut off, still paused by it, and forgets that:
/// the turn being resumed takes the goal up again, once.
pub(crate) fn take_cut_off(store: &Store, chat_id: &str, goal: &Goal) -> bool {
    let key = cut_off_key(chat_id);
    let cut_off = goal.status == GoalStatus::Paused
        && store.state(&key).as_deref() == Some(signature(goal).as_str());
    if cut_off && let Err(e) = store.set_state(&key, "") {
        log::warn!(error:% = e; "failed to forget a cut-off goal: {{error}}");
    }
    cut_off
}
