//! What a new app session does with the turns and goals the last one left behind: replies that
//! were cut off end, and their turns continue where they left off.

use std::sync::Arc;
use std::sync::PoisonError;
use std::time::Duration;

use crate::persistence::Store;
use crate::Res;
use crate::Server;
use super::TurnRequest;
use crate::types::AgentEvent;
use crate::types::ApprovalStatus;
use crate::types::GoalStatus;
use crate::types::MessageStatus;
use crate::types::now_ms;
use crate::types::Thread;
use super::goal;
use super::stream::publish;

/// `app_state` key for the threads whose turns were running when the app last quit (JSON array).
const INTERRUPTED_KEY: &str = "interrupted_turns";

/// `app_state` key prefix for when a thread's turn was last continued (JSON array of times).
const CONTINUED_KEY: &str = "continued:";

/// The message that has the agent carry on with a turn the app cut off.
const CONTINUE_PROMPT: &str = "Continue where you left off.";

/// A thread is continued at most this many times within `CONTINUE_WINDOW_MS`, so a turn that
/// keeps bringing the app down does not restart forever.
const MAX_CONTINUES: usize = 3;
const CONTINUE_WINDOW_MS: u64 = 5 * 60 * 1000;

/// How long after startup cut-off turns continue, so the window is listening for their events.
const CONTINUE_DELAY: Duration = Duration::from_secs(5);

/// Notes the threads whose turns quitting is about to stop, so the next session continues them.
pub(crate) fn remember_interrupted(store: &Store, thread_ids: &[String]) {
    let ids = serde_json::to_string(thread_ids).unwrap_or_default();
    if let Err(e) = store.set_state(INTERRUPTED_KEY, &ids) {
        log::warn!("failed to note the running turns: {e}");
    }
}

/// Finished goals remain visible for the session in which they ended. A new app session
/// dismisses them, including cached reports from the provider on its next run. Unfinished
/// goals survive, paused if their agent was interrupted by quitting, and replies that were
/// still streaming end with an error. Returns the threads whose cut-off turn can continue.
pub(crate) fn recover(store: &Store) -> Res<Vec<String>> {
    let mut interrupted: Vec<String> = store
        .state(INTERRUPTED_KEY)
        .and_then(|ids| serde_json::from_str(&ids).ok())
        .unwrap_or_default();
    store.set_state(INTERRUPTED_KEY, "[]")?;
    for mut thread in store.threads() {
        if let Some(finished) = thread.goal.as_ref().filter(|goal| {
            matches!(goal.status, GoalStatus::Complete | GoalStatus::Blocked)
        }) {
            store.dismiss_goal(&thread.id, &goal::signature(finished))?;
            thread.goal = None;
        }
        let dismissed = thread
            .goal
            .as_ref()
            .is_some_and(|g| goal::is_dismissed(store, &thread.id, g));
        let goal_active = thread
            .goal
            .as_ref()
            .is_some_and(|g| g.status == GoalStatus::Active);
        let streaming = thread
            .messages
            .iter()
            .any(|m| m.status == MessageStatus::Streaming);
        if streaming && !interrupted.contains(&thread.id) {
            interrupted.push(thread.id.clone());
        }
        if dismissed || goal_active || streaming {
            store.update_thread(&thread.id, |t| {
                if dismissed {
                    t.goal = None;
                } else if let Some(goal) =
                    t.goal.as_mut().filter(|g| g.status == GoalStatus::Active)
                {
                    goal.status = GoalStatus::Paused;
                }
                for message in &mut t.messages {
                    if message.status == MessageStatus::Streaming {
                        message.status = MessageStatus::Error;
                        message.error =
                            Some("Patchwork closed while this reply was running.".into());
                    }
                }
            })?;
        }
    }
    interrupted.retain(|id| store.thread(id).is_some_and(|t| can_continue(&t)));
    Ok(interrupted)
}

/// Whether the agent can carry on with the thread's cut-off turn: it has a session to resume,
/// and is not waiting on the user, whose answer the restart lost.
fn can_continue(thread: &Thread) -> bool {
    thread.session_id.is_some()
        && thread.archived_at.is_none()
        && thread.parent.is_none()
        && !thread
            .messages
            .iter()
            .flat_map(|m| &m.approvals)
            .any(|a| a.status == ApprovalStatus::Pending)
}

/// Records that the thread's turn continues now. False when it already has `MAX_CONTINUES`
/// times within the window.
fn note_continue(store: &Store, thread_id: &str) -> bool {
    let key = format!("{CONTINUED_KEY}{thread_id}");
    let now = now_ms();
    let mut times: Vec<u64> = store
        .state(&key)
        .and_then(|times| serde_json::from_str(&times).ok())
        .unwrap_or_default();
    times.retain(|&t| now.saturating_sub(t) < CONTINUE_WINDOW_MS);
    if times.len() >= MAX_CONTINUES {
        return false;
    }
    times.push(now);
    let saved = serde_json::to_string(&times).unwrap_or_default();
    if let Err(e) = store.set_state(&key, &saved) {
        log::warn!("failed to note a continued turn: {e}");
    }
    true
}

impl Server {
    /// Continues the turns the last app session cut off, shortly after startup. Each gets a
    /// message of its own asking the agent to carry on, shown in the thread like a typed one.
    pub fn continue_interrupted(self: &Arc<Self>) {
        let threads = std::mem::take(
            &mut *self
                .interrupted
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        if threads.is_empty() {
            return;
        }
        let server = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(CONTINUE_DELAY);
            let Some(server) = server.upgrade() else {
                return;
            };
            for thread_id in threads {
                // The user may have started a turn there already.
                if server.runner.is_running(&thread_id) || !note_continue(&server.store, &thread_id)
                {
                    continue;
                }
                let message = crate::user_message(CONTINUE_PROMPT, vec![]);
                publish(&server, &thread_id, AgentEvent::MessageStarted { message });
                let started = server.runner.start_turn(
                    &server,
                    TurnRequest {
                        thread_id,
                        text: CONTINUE_PROMPT.into(),
                        attachments: vec![],
                        images: vec![],
                    },
                );
                if let Err(e) = started {
                    log::warn!("failed to continue a cut-off turn: {e}");
                }
            }
        });
    }
}
