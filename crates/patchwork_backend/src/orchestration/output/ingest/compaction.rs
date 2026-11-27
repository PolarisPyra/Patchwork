//! Compacting an Antigravity reply that fills the chat's context window. `agy` does not compact
//! by itself, so Patchwork stops the reply between calls, sends `/compact`, and has the agent go
//! on where it was, all within the same reply.

// Rust guideline compliant 2026-11-23

use std::sync::atomic::Ordering;

use serde_json::Value;

use crate::events::ChatEvent;
use crate::orchestration::output::stream::Unsaved;
use crate::orchestration::turn::process::write_to;
use crate::orchestration::turn::start::Turn;
use crate::provider::antigravity;
use crate::system::Locked;

/// What the agent is asked once compacted, to go on with the reply it was stopped in.
const CONTINUE: &str = "Continue where you left off.";

/// What follows the compaction.
#[derive(Clone, Copy)]
pub(super) enum Then {
    /// Patchwork stopped the reply for it, so the agent goes on.
    Continue,
    /// The reply had ended by itself as it was stopped, so it ends.
    End,
}

/// Where Patchwork's compaction of the reply stands.
#[derive(Default)]
pub(super) enum Compaction {
    /// The context is under the window.
    #[default]
    Armed,
    /// The context reached the window; the reply stops once no call is running.
    Due,
    /// Patchwork asked the agent to stop the reply.
    Stopping,
    /// `/compact` is running.
    Compacting(Then),
    /// Compacted at this crossing. Armed again once the context is under the window, so a
    /// compaction that freed too little leaves the warning instead of compacting again.
    Spent,
}

/// What an end of the agent's reply means for the compaction.
pub(super) enum After {
    /// The reply ends as any other would.
    Ends,
    /// The reply was stopped to compact; this `/compact` prompt goes to the agent next.
    Compact(String),
    /// The compaction is done and the agent was asked to go on.
    GoesOn,
}

impl Compaction {
    /// Whether Patchwork is currently stopping the reply for compaction or running `/compact`.
    pub(super) fn is_compacting(&self) -> bool {
        matches!(self, Self::Stopping | Self::Compacting(_))
    }

    /// Notes the context `agy` reported, against the chat's compact `window` (0 when off).
    pub(super) fn usage(&mut self, context: u64, window: u64) {
        if window == 0 {
            return;
        }
        match self {
            Self::Armed if context >= window => *self = Self::Due,
            Self::Due | Self::Spent if context < window => *self = Self::Armed,
            _ => {}
        }
    }

    /// Stops the reply to compact it, when compaction is due and nothing would be cut short:
    /// `calls_open` says a call or swarm worker is still running.
    pub(super) fn stop_if_due(&mut self, turn: &Turn, calls_open: bool) {
        if !matches!(self, Self::Due)
            || calls_open
            || turn.cancelled.load(Ordering::SeqCst)
            || turn.idle.load(Ordering::SeqCst)
            || !turn.pending.locked().is_empty()
        {
            return;
        }
        let Some(session_id) = turn.session_id.as_deref() else {
            return;
        };
        // Under the stdin lock, as `steer` checks the flag, so a steer from here on waits.
        let mut stdin = turn.stdin.locked();
        if write_to(&mut stdin, &antigravity::interrupt(session_id)) {
            turn.compacting.store(true, Ordering::SeqCst);
            *self = Self::Stopping;
        } else {
            *self = Self::Spent;
        }
    }

    /// Moves the compaction on as the agent ends a reply. `value` is the line that ended it, and
    /// `failed` says the reply reported an error, which ends the compaction with the reply.
    pub(super) fn finished(
        &mut self,
        turn: &Turn,
        value: &Value,
        failed: bool,
        unsaved: &mut Unsaved,
    ) -> After {
        let cancelled = turn.cancelled.load(Ordering::SeqCst);
        match std::mem::take(self) {
            Self::Stopping if !failed && !cancelled => {
                let Some(session_id) = turn.session_id.as_deref() else {
                    self.stopped(turn);
                    return After::Ends;
                };
                let then = if value["result"]["stopReason"] == "cancelled" {
                    Then::Continue
                } else {
                    Then::End
                };
                turn.save_text(unsaved);
                turn.publish(ChatEvent::Compacting {
                    message_id: turn.message_id.clone(),
                });
                *self = Self::Compacting(then);
                After::Compact(antigravity::prompt_input(session_id, "/compact", &[], &[]))
            }
            Self::Compacting(then) if !failed && !cancelled => {
                turn.publish(ChatEvent::Compacted {
                    message_id: turn.message_id.clone(),
                    summary: None,
                });
                *self = Self::Spent;
                let mut stdin = turn.stdin.locked();
                turn.compacting.store(false, Ordering::SeqCst);
                // Messages sent meanwhile go on from here as steers do, in place of the prompt.
                let goes_on = matches!(then, Then::Continue)
                    && turn.steers.locked().is_empty()
                    && turn.session_id.as_deref().is_some_and(|id| {
                        write_to(
                            &mut stdin,
                            &antigravity::prompt_input(id, CONTINUE, &[], &[]),
                        )
                    });
                if goes_on { After::GoesOn } else { After::Ends }
            }
            state @ (Self::Stopping | Self::Compacting(_)) => {
                *self = state;
                self.stopped(turn);
                After::Ends
            }
            state => {
                *self = state;
                After::Ends
            }
        }
    }

    /// Ends a compaction under way, as when the user stops the reply; nothing goes on after it.
    pub(super) fn stopped(&mut self, turn: &Turn) {
        turn.compacting.store(false, Ordering::SeqCst);
        if matches!(self, Self::Stopping | Self::Compacting(_)) {
            *self = Self::Spent;
        }
    }
}
