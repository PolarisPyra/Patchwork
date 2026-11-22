//! A goal while turns pursue it: what the agent is told, how each turn's end settles the goal,
//! and the message that carries it on. Patchwork runs the goal the same way for every agent.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use crate::Core;
use crate::events::AgentEvent;
use crate::orchestration::turn::start::Turn;
use crate::persistence::Store;
use super::state::stalled;
use crate::system::now_ms;
use crate::provider::{GoalCommand, goal_command};
use crate::chats::{Goal, GoalStatus, MessageStatus};

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

/// How the message that carries a goal on starts.
const CONTINUE: &str = "Continue working toward the goal: ";

/// What carries the goal on after a reply that did not end it.
fn carry_on(objective: &str) -> String {
    format!(
        "{CONTINUE}{objective}\n\n\
         End your reply with {MET} once it is met and checked, or {BLOCKED}: and the reason."
    )
}

/// Whether `text` is the message Patchwork sent to carry a goal on, which the chat does not show.
#[must_use]
pub fn is_goal_continuation(text: &str) -> bool {
    text.starts_with(CONTINUE)
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
    turn.publish(AgentEvent::GoalUpdated { goal: Some(goal) });
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
