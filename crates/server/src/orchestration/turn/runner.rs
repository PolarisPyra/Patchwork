//! The `Runner`: the agent processes up in each thread, and handing a message to one, stopping
//! one, or stopping them all.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::orchestration::changes::attribution::{self, SharedAttribution};
use crate::orchestration::turn::process::{
    INTERRUPT_GRACE, QUIT_GRACE, terminate, write_line, write_to,
};
use crate::orchestration::messages::queue::{FollowUp, FollowUpInput};
use crate::orchestration::turn::start::RunningTurn;
use crate::permissions::Target;
use crate::provider;
use crate::system::scope;
use crate::workspace;

/// What became of a message sent to a thread.
#[derive(Debug)]
pub enum Delivery {
    /// Written to the running turn.
    Steered,
    /// Waiting for the running turn to end.
    Queued,
    /// Written to an idle agent, where it starts a new reply.
    Resumed,
    /// No turn is running.
    NotRunning,
}

#[derive(Default)]
pub struct Runner {
    pub(in crate::orchestration) turns: Mutex<HashMap<String, RunningTurn>>,
    /// Threads whose turn is being started. Its agent is not in `turns` until it is spawned, so a
    /// second start in that window would run a second agent beside the first.
    pub(in crate::orchestration) starting: Mutex<HashSet<String>>,
    /// Messages this process has handed to a turn, whether steered or queued. Written under the
    /// `turns` lock, so a withdrawal sees it in the same order as the queue it checks.
    pub(in crate::orchestration) handed_over: Mutex<HashSet<String>>,
    /// Threads stopped while their turn was starting, whose agent is stopped once it is up.
    pub(in crate::orchestration) stop_starting: Mutex<HashSet<String>>,
    /// Permission hooks waiting for the user, by approval id.
    pub(in crate::orchestration) hooks: Mutex<HashMap<String, std::sync::mpsc::Sender<bool>>>,
    /// Calls the user allowed through a hook, by thread, so the agent's own request for them
    /// is not asked again.
    pub(in crate::orchestration) grants: Mutex<HashMap<String, Vec<Target>>>,
    /// The commands Patchwork's own agent runs, which outlive its turns as background jobs.
    pub(in crate::orchestration) jobs: crate::orchestration::tasks::jobs::Jobs,
}

impl std::fmt::Debug for Runner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runner").finish_non_exhaustive()
    }
}

impl Runner {
    /// Whether a turn is being started in the thread, before its agent is in `turns`.
    pub fn is_starting(&self, thread_id: &str) -> bool {
        self.starting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(thread_id)
    }

    /// Asks a turn still starting to stop once its agent is up. Called under the `turns` lock,
    /// which the start inserts its agent under before it looks here.
    fn stop_when_started(&self, thread_id: &str) {
        if self.is_starting(thread_id) {
            self.stop_starting
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(thread_id.into());
        }
    }

    /// Whether a reply is under way in the thread. An agent that is only kept up by shell
    /// commands it left running does not count.
    pub fn is_running(&self, thread_id: &str) -> bool {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(thread_id)
            .is_some_and(|turn| !turn.idle.load(Ordering::SeqCst))
    }

    /// Whether an agent process is up in the thread, replying or idle.
    pub fn has_agent(&self, thread_id: &str) -> bool {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(thread_id)
    }

    /// The threads with a reply under way, each with what its files are judged by.
    pub(in crate::orchestration) fn attributions(&self) -> Vec<(String, SharedAttribution)> {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        turns
            .iter()
            .filter(|(_, turn)| !turn.idle.load(Ordering::SeqCst))
            .map(|(id, turn)| (id.clone(), Arc::clone(&turn.attribution)))
            .collect()
    }

    /// The threads with a reply under way.
    pub fn running(&self) -> Vec<String> {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        turns
            .iter()
            .filter(|(_, turn)| !turn.idle.load(Ordering::SeqCst))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Stops every idle agent, as `retire_idle` does.
    pub fn retire_all_idle(&self) {
        let threads: Vec<String> = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            turns.keys().cloned().collect()
        };
        for thread_id in threads {
            self.retire_idle(&thread_id);
        }
    }

    /// Stops the thread's idle agent, and the shell commands it left running, so a turn that
    /// needs a process of its own can start. A turn that is replying is left to finish, and is
    /// not used again once it is idle.
    pub fn retire_idle(&self, thread_id: &str) {
        let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        match turns.get(thread_id) {
            Some(turn) if turn.idle.load(Ordering::SeqCst) => {}
            Some(turn) => {
                turn.stale.store(true, Ordering::SeqCst);
                return;
            }
            None => return,
        }
        let Some(turn) = turns.remove(thread_id) else {
            return;
        };
        turn.cancelled.store(true, Ordering::SeqCst);
        std::thread::spawn(move || {
            terminate(turn.pid, turn.exited);
            if let Some(unit) = turn.scope {
                scope::stop(&unit);
            }
        });
    }

    /// Starts the idle agent's record of the project's files over from how they are now, before
    /// a message resumes it: the watcher leaves an idle agent alone, so what changed between its
    /// replies, such as the user's own edits, is no part of the next one.
    pub(in crate::orchestration) fn rebase_idle(&self, thread_id: &str) {
        let found = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            turns
                .get(thread_id)
                .filter(|turn| turn.idle.load(Ordering::SeqCst))
                .map(|turn| {
                    (
                        turn.root.clone(),
                        Arc::clone(&turn.idle),
                        Arc::clone(&turn.attribution),
                    )
                })
        };
        let Some((root, idle, shared)) = found else {
            return;
        };
        let now = workspace::git_changes(&root).unwrap_or_default();
        let mut record = attribution::lock(&shared);
        // Another message may have resumed it meanwhile, and that reply's record stays.
        if idle.load(Ordering::SeqCst) {
            record.settle(now);
        }
    }

    /// Hands a new compact window to the API turn replying in `thread_id`, which compacts by it
    /// from its next step. Other agents read it only when they start.
    pub fn set_compact_window(&self, thread_id: &str, window: u64) {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(turn) = turns.get(thread_id)
            && turn.agent == provider::AgentKind::Api
            && !turn.idle.load(Ordering::SeqCst)
        {
            write_line(&turn.stdin, &provider::api::compact_window_line(window));
        }
    }

    /// Hands a message to the turn running in `thread_id`: written to the agent when it
    /// can take input mid-turn, otherwise kept to start the next turn.
    /// `subagents` are the subagent calls still running in the foreground.
    pub fn steer(
        &self,
        thread_id: &str,
        input: FollowUpInput<'_>,
        session_id: Option<&str>,
        subagents: &[String],
    ) -> Delivery {
        let FollowUpInput {
            message_id,
            text,
            attachments,
            images,
        } = input;
        self.background_subagents(thread_id, subagents);
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(turn) = turns.get(thread_id) else {
            return Delivery::NotRunning;
        };
        let follow_up = FollowUp {
            message_id: message_id.into(),
            text: text.into(),
            attachments: attachments.to_vec(),
            images: images.to_vec(),
        };
        self.handed_over
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(message_id.into());
        // A stopped turn takes nothing more; the message starts the next turn.
        if turn.cancelled.load(Ordering::SeqCst) {
            turn.queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(follow_up);
            return Delivery::Queued;
        }
        let turn_id = turn
            .native
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let prompt = provider::Prompt {
            text,
            attachments,
            images,
        };
        let line = provider::steer_input(turn.agent, prompt, session_id, turn_id.as_deref());
        // Under the stdin lock, so the reader cannot close stdin between the write and
        // recording that an answer is due.
        let mut stdin = turn.stdin.lock().unwrap_or_else(PoisonError::into_inner);
        if turn.idle.load(Ordering::SeqCst) {
            // The agent's last reply is over; this message starts the next one, and if the agent
            // can no longer take it, or was started with settings changed since, a new turn
            // replaces the idle process.
            if !turn.stale.load(Ordering::SeqCst)
                && line.is_some_and(|line| write_to(&mut stdin, &line))
            {
                turn.idle.store(false, Ordering::SeqCst);
                turn.steers
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push_back(follow_up);
                return Delivery::Resumed;
            }
            self.handed_over
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(message_id);
            return Delivery::NotRunning;
        }
        if let Some(line) = line
            && write_to(&mut stdin, &line)
        {
            turn.steers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push_back(follow_up);
            return Delivery::Steered;
        }
        turn.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(follow_up);
        Delivery::Queued
    }

    /// Stops the turn and drops the messages waiting for it. `last_line` is written
    /// to the agent first, with a moment to act on it. The agent's scope is stopped along
    /// with its process group, so a command it started in a session of its own, which
    /// could keep its output open, ends with it.
    pub fn cancel(&self, thread_id: &str, last_line: Option<String>) {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(turn) = turns.get(thread_id) {
            stop_turn(turn);
            let (pid, exited, scope) = (turn.pid, Arc::clone(&turn.exited), turn.scope.clone());
            let grace = if last_line.is_some_and(|line| write_line(&turn.stdin, &line)) {
                // Time for the agent to act on its last line before it is stopped.
                Duration::from_millis(300)
            } else {
                Duration::ZERO
            };
            std::thread::spawn(move || {
                std::thread::sleep(grace);
                terminate(pid, exited);
                if let Some(unit) = scope {
                    scope::stop(&unit);
                }
            });
        } else {
            self.stop_when_started(thread_id);
        }
    }

    /// Stops the reply under way and leaves the messages waiting for it queued, as `cancel` does, but
    /// by interrupting the agent, which ends its turn cleanly in its session. Claude then stays
    /// up for the next message. An agent that cannot be interrupted, or that does not end its
    /// reply within `INTERRUPT_GRACE`, is stopped as `cancel` stops it. `session_id` is the
    /// thread's agent session, which Codex needs.
    pub fn interrupt(&self, thread_id: &str, session_id: Option<&str>, last_line: Option<String>) {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(turn) = turns.get(thread_id) else {
            self.stop_when_started(thread_id);
            return;
        };
        let turn_id = turn
            .native
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        // An idle agent has no reply to stop; it is stopped with the commands it left running.
        let line = (!turn.idle.load(Ordering::SeqCst))
            .then(|| provider::interrupt(turn.agent, session_id, turn_id.as_deref()))
            .flatten();
        let Some(line) = line else {
            drop(turns);
            return self.cancel(thread_id, last_line);
        };
        stop_turn(turn);
        if let Some(last_line) = &last_line {
            write_line(&turn.stdin, last_line);
        }
        let asked = turn.interrupts.ask();
        let wait = if write_line(&turn.stdin, &line) {
            INTERRUPT_GRACE
        } else {
            Duration::ZERO
        };
        let (pid, exited, scope) = (turn.pid, Arc::clone(&turn.exited), turn.scope.clone());
        let interrupts = Arc::clone(&turn.interrupts);
        std::thread::spawn(move || {
            let deadline = Instant::now() + wait;
            let done = || exited.load(Ordering::SeqCst) || interrupts.answered(asked);
            while !done() && Instant::now() < deadline {
                // How often the stop checks whether the agent exited or answered, while it waits.
                std::thread::sleep(Duration::from_millis(50));
            }
            if !done() {
                terminate(pid, exited);
                if let Some(unit) = scope {
                    scope::stop(&unit);
                }
            }
        });
    }

    /// Stops every running agent, so none outlives the app when it quits. Agents that
    /// are still running after `QUIT_GRACE` are sent SIGKILL.
    pub fn stop_all(&self) {
        let running: Vec<(u32, Arc<AtomicBool>)> = self
            .turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|turn| {
                turn.cancelled.store(true, Ordering::SeqCst);
                (turn.pid, Arc::clone(&turn.exited))
            })
            .collect();
        for (pid, _) in &running {
            workspace::signal_process_group(*pid, workspace::SIGTERM);
        }
        let deadline = Instant::now() + QUIT_GRACE;
        while Instant::now() < deadline && running.iter().any(|(_, e)| !e.load(Ordering::SeqCst)) {
            // How often quitting checks whether every agent has exited, while it waits.
            std::thread::sleep(Duration::from_millis(50));
        }
        running
            .iter()
            .filter(|(_, exited)| !exited.load(Ordering::SeqCst))
            .for_each(|(pid, _)| workspace::signal_process_group(*pid, workspace::SIGKILL));
    }
}

/// Marks the turn stopped. The messages waiting for it stay queued, and the turn's end takes
/// them off its queue.
fn stop_turn(turn: &RunningTurn) {
    turn.cancelled.store(true, Ordering::SeqCst);
}
