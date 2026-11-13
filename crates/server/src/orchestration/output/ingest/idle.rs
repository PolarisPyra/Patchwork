//! The agent going idle between messages: whether it can, and the turn's end when it does.

// Rust guideline compliant 2026-10-09

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use crate::hooks::HookEvent;
use crate::events::AgentEvent;
use crate::provider::AgentKind;
use crate::threads::Goal;
use crate::threads::GoalStatus;
use crate::threads::MessageStatus;
use crate::workspace;

use crate::orchestration::changes::attribution;
use crate::orchestration::output::stream::Unsaved;
use crate::orchestration::messages::queue::FollowUp;
use crate::orchestration::turn::end::{hold_unanswered, turn_hooks, record_turn};
use crate::orchestration::turn::start::Turn;

/// Whether Claude, having ended its reply cleanly, can stay up for the next message.
pub(super) fn can_idle(turn: &Turn, goal: Option<&Goal>, stream_error: Option<&String>) -> bool {
    stream_error.is_none() && !turn.cancelled.load(Ordering::SeqCst) && stays_up(turn, goal)
}

/// Whether the agent's process can stay up once its reply has ended. Only Claude keeps its process
/// between messages, and a goal still to be pursued keeps the turn going instead.
pub(super) fn stays_up(turn: &Turn, goal: Option<&Goal>) -> bool {
    turn.agent == AgentKind::Claude
        && !goal.is_some_and(|g| matches!(g.status, GoalStatus::Active | GoalStatus::Paused))
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
    let after = workspace::git_changes(&turn.root).unwrap_or_default();
    let changed = attribution::lock(&turn.attribution).settle(after);
    for path in changed {
        turn.publish(AgentEvent::FileTouched {
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
                (turn.server.runner.stopped_status(), None)
            } else if at == last && error.is_some() {
                (MessageStatus::Error, error.map(String::from))
            } else {
                (MessageStatus::Complete, None)
            };
            turn.publish(AgentEvent::TurnCompleted {
                message_id: id.clone(),
                status,
                error,
            });
        });
    *completed = segments.len();
    if stopped {
        // The interrupt ended the requests the agent was waiting on, and the steering messages
        // it had not taken. A permission hook still holding a call hears it was refused.
        let lapsed: Vec<String> = turn
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
            .map(|(id, _)| id)
            .collect();
        let mut hooks = turn
            .server
            .runner
            .hooks
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        lapsed
            .iter()
            .filter_map(|id| hooks.remove(id))
            .for_each(|waiter| {
                let _ = waiter.send(false);
            });
        drop(hooks);
        // Steers the agent never took are held as queued messages, as the turn's end holds them.
        let unanswered: Vec<String> = turn
            .steers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .flat_map(|f| f.ids().collect::<Vec<_>>())
            .collect();
        hold_unanswered(turn, &unanswered);
        let (server, thread_id, root, agent) = (
            Arc::clone(&turn.server),
            turn.thread_id.clone(),
            turn.root.clone(),
            turn.agent,
        );
        std::thread::spawn(move || {
            let status = Some(MessageStatus::Cancelled);
            turn_hooks(
                &server,
                HookEvent::Interrupt,
                &thread_id,
                &root,
                agent,
                status,
            );
        });
    }
    // Under the turns lock, where `steer` reads these flags: a message sent while the reply was
    // wrapping up is in the queue taken here, not left in one nobody drains.
    let turns = turn
        .server
        .runner
        .turns
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let queued = std::mem::take(&mut *turn.queue.lock().unwrap_or_else(PoisonError::into_inner));
    let mut handed_over = turn
        .server
        .runner
        .handed_over
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    for message in &queued {
        handed_over.remove(&message.message_id);
    }
    drop(handed_over);
    // A call the user allowed through a hook was allowed for this reply only.
    turn.server
        .runner
        .grants
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&turn.thread_id);
    if stopped {
        // The agent takes the next message again.
        turn.cancelled.store(false, Ordering::SeqCst);
    }
    turn.idle.store(true, Ordering::SeqCst);
    drop(turns);
    // Taken from the queue, so marked queued here: the sender may not have saved the flag yet, and
    // the queued messages are sent, or wait for the user, by that flag.
    hold_unanswered(
        turn,
        &queued.iter().flat_map(FollowUp::ids).collect::<Vec<_>>(),
    );
    if !stopped {
        turn.server.send_queued_later(&turn.thread_id);
    }
}
