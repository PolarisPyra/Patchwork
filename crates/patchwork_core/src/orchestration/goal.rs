//! A chat's `/goal`: what the app keeps about it, how turns pursue it, and stopping it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use crate::{Error, Core, orchestration};
use crate::persistence::Store;
use crate::system::now_ms;
use crate::chats::{Goal, GoalStatus, MessageStatus};
use crate::events::ChatEvent;
use crate::orchestration::turn::start::Turn;
use crate::provider::{GoalCommand, goal_command};
pub use patchwork_model::goal::{CONTINUE, Upkeep, is_goal_continuation, is_upkeep, upkeep};

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

/// The line a reply ends with once the goal is met.
const MET: &str = "GOAL MET";

/// The start of the line a reply ends with when the goal cannot be met without the user.
const BLOCKED: &str = "GOAL BLOCKED";

/// What the agent is told to work toward `objective`, and how to say where it stands.
fn brief(objective: &str) -> String {
    format!(
        "Work toward this goal until it is fully met:\n\n{objective}\n\n\
         After each of your replies you are asked to continue, until a reply ends with a line \
         that reads {MET} once the goal is met and checked, or {BLOCKED}: and the reason when it \
         cannot be met without the user."
    )
}

/// What carries the goal on after a reply that did not end it.
fn carry_on(objective: &str) -> String {
    format!(
        "{CONTINUE}{objective}\n\n\
         End your reply with {MET} once it is met and checked, or {BLOCKED}: and the reason."
    )
}

/// The text the agent gets for `text`: a `/goal` that sets or resumes the goal becomes its brief,
/// and any other message is unchanged.
pub(crate) fn agent_text(store: &Store, chat_id: &str, text: String) -> String {
    match goal_command(&text) {
        Some(GoalCommand::Set(objective)) => brief(objective),
        Some(GoalCommand::Resume) => store
            .chat(chat_id)
            .and_then(|t| t.goal)
            .map_or(text, |g| brief(&g.objective)),
        _ => text,
    }
}

/// Where a reply leaves the goal, from the line it ends with: met, blocked, or still active.
fn verdict(reply: &str) -> GoalStatus {
    let last = reply
        .lines()
        .rev()
        .map(|line| line.trim().trim_matches(['*', '_', '`']).trim())
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    if last.eq_ignore_ascii_case(MET) {
        GoalStatus::Complete
    } else if last
        .get(..BLOCKED.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(BLOCKED))
    {
        GoalStatus::Blocked
    } else {
        GoalStatus::Active
    }
}

/// Settles the goal once a turn ends, from how it ended and the line the last of its `replies`
/// ends with. A goal whose replies twice in a row used no tools is going nowhere, so it pauses.
/// Returns whether the goal goes on in another turn.
pub(in crate::orchestration) fn settle(
    turn: &Turn,
    status: MessageStatus,
    cancelled: bool,
    replies: &[String],
) -> bool {
    // The stored goal is read again, since the user may have paused or cleared it meanwhile.
    let chat = turn.core.store.chat(&turn.chat_id);
    let Some(goal) = chat
        .as_ref()
        .and_then(|t| t.goal.clone())
        .filter(|g| g.status == GoalStatus::Active)
    else {
        return false;
    };
    let next = if cancelled || status != MessageStatus::Complete {
        // Stopped, failed or killed: nothing pursues the goal, so it waits to be resumed.
        GoalStatus::Paused
    } else {
        let replies: Vec<_> = chat
            .as_ref()
            .map(|t| {
                t.messages
                    .iter()
                    .filter(|m| replies.contains(&m.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let last = replies.last().map(|m| m.text.as_str()).unwrap_or_default();
        let idle = replies.iter().all(|m| m.tools.is_empty());
        match verdict(last) {
            GoalStatus::Active if stalled(&turn.core.store, &turn.chat_id, &goal, idle) => {
                GoalStatus::Stalled
            }
            verdict => verdict,
        }
    };
    let goal = Goal {
        checks: Some(goal.checks.unwrap_or(0) + 1),
        ..goal.with_status(next, now_ms())
    };
    turn.publish(ChatEvent::GoalUpdated { goal: Some(goal) });
    next == GoalStatus::Active
}

/// Once the turn has ended, sends the messages queued meanwhile or, with none queued, the one
/// that carries the goal on.
pub(in crate::orchestration) fn carry_on_later(core: &Arc<Core>, chat_id: &str) {
    let core = Arc::clone(core);
    let chat_id = chat_id.to_string();
    std::thread::spawn(move || {
        let Some(chat) = core.store.chat(&chat_id) else {
            return;
        };
        let sent = if chat.messages.iter().any(|m| m.queued) {
            core.resume_queued(&chat_id)
        } else if let Some(goal) = chat.goal.filter(|g| g.status == GoalStatus::Active) {
            core.send_message(chat_id, carry_on(&goal.objective), vec![], false)
                .map(|_| ())
        } else {
            Ok(())
        };
        if let Err(e) = sent {
            log::warn!(error:% = e; "failed to carry the goal on: {{error}}");
        }
    });
}

impl Core {
    /// Clears the goal. A reply still running finishes, and nothing carries the goal on after it.
    ///
    /// # Errors
    ///
    /// Returns an error when the goal's dismissal cannot be saved.
    pub fn stop_goal(self: &Arc<Self>, chat_id: &str) -> Result<(), Error> {
        let Some(goal) = self.store.chat(chat_id).and_then(|t| t.goal) else {
            return Ok(());
        };
        self.store.dismiss_goal(chat_id, &signature(&goal))?;
        orchestration::publish(self, chat_id, ChatEvent::GoalUpdated { goal: None });
        Ok(())
    }

    /// Pauses the goal. A reply still running finishes, and nothing carries the goal on after it
    /// until it is resumed.
    pub(crate) fn pause_goal(self: &Arc<Self>, chat_id: &str) {
        let Some(goal) = self
            .store
            .chat(chat_id)
            .and_then(|t| t.goal)
            .filter(|g| g.status == GoalStatus::Active)
        else {
            return;
        };
        orchestration::publish(
            self,
            chat_id,
            ChatEvent::GoalUpdated {
                goal: Some(goal.with_status(GoalStatus::Paused, now_ms())),
            },
        );
    }
}
