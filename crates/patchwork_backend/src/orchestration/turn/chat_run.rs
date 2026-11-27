//! `ChatRun`: everything the runner keeps of one chat's run, changed under one lock.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;

use crate::orchestration::capabilities::Ran;
use crate::orchestration::tasks::delegate::Called;
use crate::orchestration::turn::start::RunningTurn;
use crate::permissions::Target;

/// One chat's run: its agent process, a turn starting, and what the turn was handed or allowed.
///
/// The runner keeps one per chat that has any of these, and drops it once it holds none, so a
/// chat with nothing running costs nothing.
#[derive(Default)]
pub(in crate::orchestration) struct ChatRun {
    /// The agent process up in the chat, replying or idle.
    pub(in crate::orchestration) turn: Option<RunningTurn>,
    /// A turn is being started. Its agent is not in `turn` until it is spawned, so a second
    /// start in that window would run a second agent beside the first.
    pub(in crate::orchestration) starting: bool,
    /// Stopped while the turn was starting: its agent is stopped once it is up.
    pub(in crate::orchestration) stop_when_up: bool,
    /// Messages handed to the turn, whether steered or queued. Changed together with the turn's
    /// queue, so a withdrawal sees both in the same order.
    pub(in crate::orchestration) handed_over: HashSet<String>,
    /// Permission hooks waiting for the user, by approval id.
    pub(in crate::orchestration) hooks: HashMap<String, Sender<bool>>,
    /// Calls the user allowed through a hook, so the agent's own request for them is not asked
    /// again.
    pub(in crate::orchestration) grants: Vec<Target>,
    /// Calls that start a sub agent on another agent, from when the reply shows one until the
    /// sub agent it starts takes it up.
    pub(in crate::orchestration) delegated: Vec<Called>,
    /// What the chat app's dispatcher had Patchwork do in its turn.
    pub(in crate::orchestration) ran: Ran,
}

impl ChatRun {
    /// Whether the run holds nothing worth keeping.
    pub(in crate::orchestration) fn is_empty(&self) -> bool {
        self.turn.is_none()
            && !self.starting
            && !self.stop_when_up
            && self.handed_over.is_empty()
            && self.hooks.is_empty()
            && self.grants.is_empty()
            && self.delegated.is_empty()
            && self.ran.is_idle()
    }

    /// Lets go of what only the turn that ended needed: the messages it was handed, its hooks,
    /// which then hear they were refused, its grants, and its sub agent calls. Kept while a new
    /// turn starts or runs, which may have been handed some of them.
    pub(in crate::orchestration) fn forget_ended_turn(&mut self) {
        if self.turn.is_some() || self.starting {
            return;
        }
        self.handed_over.clear();
        self.hooks.clear();
        self.grants.clear();
        self.delegated.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_run_is_empty() {
        assert!(ChatRun::default().is_empty());
    }

    #[test]
    fn any_state_keeps_the_run() {
        let starting = ChatRun {
            starting: true,
            ..ChatRun::default()
        };
        assert!(!starting.is_empty());
        let mut handed = ChatRun::default();
        handed.handed_over.insert("m-1".into());
        assert!(!handed.is_empty());
        let mut granted = ChatRun::default();
        granted.grants.push(Target::new(
            crate::permissions::PermissionKind::Command,
            ["ls"],
        ));
        assert!(!granted.is_empty());
    }

    #[test]
    fn ended_turn_is_forgotten_unless_another_starts() {
        let mut run = ChatRun::default();
        run.handed_over.insert("m-1".into());
        run.grants.push(Target::new(
            crate::permissions::PermissionKind::Command,
            ["ls"],
        ));
        run.starting = true;
        run.forget_ended_turn();
        assert!(!run.handed_over.is_empty(), "a starting turn keeps them");
        run.starting = false;
        run.forget_ended_turn();
        assert!(run.is_empty());
    }
}
