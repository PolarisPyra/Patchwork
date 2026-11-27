//! The agent going idle between messages: whether it can, and the turn's end when it does.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::extensions::hooks::HookEvent;
use crate::events::ChatEvent;
use crate::provider::{ProviderKind, Steer};
use crate::chats::Goal;
use crate::chats::GoalStatus;
use crate::chats::MessageStatus;
use crate::source_control;

use crate::orchestration::output::stream::Unsaved;
use crate::orchestration::messages::queue::FollowUp;
use crate::orchestration::turn::end::{hold_unanswered, turn_hooks, record_turn};
use crate::orchestration::turn::start::Turn;

/// Whether Claude, having ended its reply cleanly, can stay up for the next message.
pub(super) fn can_idle(turn: &Turn, goal: Option<&Goal>, stream_error: Option<&String>) -> bool {
    stream_error.is_none() && !turn.cancelled.load(Ordering::SeqCst) && stays_up(turn, goal)
}

/// Whether the agent's process can stay up once its reply has ended. Agents that can keep their
/// process warm between messages so the next turn skips cold startup. An active goal ends the
/// turn instead, so the goal is settled and carried on.
pub(super) fn stays_up(turn: &Turn, goal: Option<&Goal>) -> bool {
    turn.agent.capabilities().stays_up && !goal.is_some_and(|g| g.status == GoalStatus::Active)
}

/// Completes the replies not yet completed, with the files they changed, and marks the turn
/// idle: the agent stays up and takes the next message as a new reply, starting with the
/// messages queued meanwhile. `stopped` when Stop interrupted the last reply: it ends as
/// stopped, its unanswered requests lapse, and the queued messages wait for the user. `error`
/// when the last reply failed: it ends as failed, with it.
pub(super) fn go_idle(
    turn: &Turn,
    segments: &[String],
    completed: &mut usize,
    unsaved: &mut Unsaved,
    stopped: bool,
    error: Option<&str>,
) {
    turn.save_text(unsaved);
    let after = source_control::git_changes(&turn.root).unwrap_or_default();
    let changed = turn.attribution.locked().settle(&turn.root, after);
    for path in changed {
        turn.publish(ChatEvent::FileTouched {
            message_id: turn.message_id.clone(),
            path,
        });
    }
    record_turn(turn, &segments[*completed..]);
    let last = segments.len().saturating_sub(1);
    segments
        .iter()
        .enumerate()
        .skip(*completed)
        .for_each(|(at, id)| {
            let (status, error) = if stopped && at == last {
                (turn.core.runner.stopped_status(), None)
            } else if at == last && error.is_some() {
                (MessageStatus::Error, error.map(String::from))
            } else {
                (MessageStatus::Complete, None)
            };
            turn.publish(ChatEvent::TurnCompleted {
                message_id: id.clone(),
                status,
                error,
            });
        });
    *completed = segments.len();
    if stopped {
        // The interrupt ended the requests the agent was waiting on, and the steering messages
        // it had not taken. A permission hook still holding a call hears it was refused.
        let lapsed: Vec<String> = turn.pending.locked().drain().map(|(id, _)| id).collect();
        let waiters: Vec<_> = turn.core.runner.with_run(&turn.chat_id, |run| {
            lapsed
                .iter()
                .filter_map(|id| run.hooks.remove(id))
                .collect()
        });
        for waiter in waiters {
            let _ = waiter.send(false);
        }
        // Steers the agent never took are held as queued messages, as the turn's end holds them.
        let unanswered: Vec<String> = turn
            .steers
            .locked()
            .drain(..)
            .flat_map(|f| f.ids().collect::<Vec<_>>())
            .collect();
        hold_unanswered(turn, &unanswered);
        let (core, chat_id, root, agent) = (
            Arc::clone(&turn.core),
            turn.chat_id.clone(),
            turn.root.clone(),
            turn.agent,
        );
        std::thread::spawn(move || {
            let status = Some(MessageStatus::Cancelled);
            turn_hooks(&core, HookEvent::Interrupt, &chat_id, &root, agent, status);
        });
    }
    // Under the runner's lock, where `steer` reads these flags: a message sent while the reply
    // was wrapping up is in the queue taken here, not left in one nobody drains.
    let queued = turn.core.runner.with_run(&turn.chat_id, |run| {
        let mut queued = std::mem::take(&mut *turn.queue.locked());
        // A steer that stopped the reply as it ended stopped nothing, so it goes first.
        if turn.agent.capabilities().steer == Steer::StopsReply {
            queued.splice(0..0, turn.steers.locked().drain(..));
            turn.cancelling.store(false, Ordering::SeqCst);
        }
        for message in &queued {
            run.handed_over.remove(&message.message_id);
        }
        // A call the user allowed through a hook was allowed for this reply only.
        run.grants.clear();
        if stopped {
            // The agent takes the next message again.
            turn.cancelled.store(false, Ordering::SeqCst);
        }
        turn.idle.store(true, Ordering::SeqCst);
        queued
    });
    // Taken from the queue, so marked queued here: the sender may not have saved the flag yet, and
    // the queued messages are sent, or wait for the user, by that flag.
    hold_unanswered(
        turn,
        &queued.iter().flat_map(FollowUp::ids).collect::<Vec<_>>(),
    );
    if !stopped {
        turn.core.send_queued_later(&turn.chat_id);
    }
}
