//! The `Runner`: the agent processes up in each chat, and handing a message to one, stopping
//! one, or stopping them all.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::orchestration::changes::attribution::SharedAttribution;
use crate::orchestration::turn::process::{
    INTERRUPT_GRACE, QUIT_GRACE, terminate, write_line, write_to,
};
use crate::orchestration::messages::queue::{FollowUp, FollowUpInput};
use crate::orchestration::turn::limit::LimitWait;
use crate::orchestration::turn::start::RunningTurn;
use crate::permissions::Target;
use crate::provider;
use crate::system::processes::{running, scope};
use crate::chats::MessageStatus;
use crate::source_control;

/// What became of a message sent to a chat.
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
    /// Chats whose turn is being started. Its agent is not in `turns` until it is spawned, so a
    /// second start in that window would run a second agent beside the first.
    pub(in crate::orchestration) starting: Mutex<HashSet<String>>,
    /// Messages this process has handed to a turn, whether steered or queued. Written under the
    /// `turns` lock, so a withdrawal sees it in the same order as the queue it checks.
    pub(in crate::orchestration) handed_over: Mutex<HashSet<String>>,
    /// Chats stopped while their turn was starting, whose agent is stopped once it is up.
    pub(in crate::orchestration) stop_starting: Mutex<HashSet<String>>,
    /// Permission hooks waiting for the user, by approval id.
    pub(in crate::orchestration) hooks: Mutex<HashMap<String, std::sync::mpsc::Sender<bool>>>,
    /// Calls the user allowed through a hook, by chat, so the agent's own request for them
    /// is not asked again.
    pub(in crate::orchestration) grants: Mutex<HashMap<String, Vec<Target>>>,
    /// The commands Patchwork's own agent runs, which outlive its turns as background jobs.
    pub(in crate::orchestration) jobs: crate::orchestration::tasks::jobs::Jobs,
    /// The turns that resume when their agent's usage limit resets, by chat.
    pub(in crate::orchestration) limit_waits: Mutex<HashMap<String, LimitWait>>,
    /// Calls that start a sub agent on another agent, by the chat that made them, from when
    /// its reply shows one until the sub agent it starts takes it up.
    pub(in crate::orchestration) delegated:
        Mutex<HashMap<String, Vec<crate::orchestration::tasks::delegate::Called>>>,
    /// What each chat app's dispatcher had Patchwork do in its turn, by its chat.
    pub(in crate::orchestration) ran:
        Mutex<HashMap<String, crate::orchestration::capabilities::Ran>>,
    /// Set once the app is quitting. A turn stopped from then on was cut off by the quit, not
    /// stopped by the user, so it ends interrupted and resumes at the next launch.
    quitting: AtomicBool,
}

impl std::fmt::Debug for Runner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runner").finish_non_exhaustive()
    }
}

impl Runner {
    /// Whether a turn is being started in the chat, before its agent is in `turns`.
    pub fn is_starting(&self, chat_id: &str) -> bool {
        self.starting.locked().contains(chat_id)
    }

    /// Asks a turn still starting to stop once its agent is up. Called under the `turns` lock,
    /// which the start inserts its agent under before it looks here.
    fn stop_when_started(&self, chat_id: &str) {
        if self.is_starting(chat_id) {
            self.stop_starting.locked().insert(chat_id.into());
        }
    }

    /// Whether a reply is under way in the chat. An agent that is only kept up by shell
    /// commands it left running does not count.
    pub fn is_running(&self, chat_id: &str) -> bool {
        self.turns
            .locked()
            .get(chat_id)
            .is_some_and(|turn| !turn.idle.load(Ordering::SeqCst))
    }

    /// Whether an agent process is up in the chat, replying or idle.
    pub fn has_agent(&self, chat_id: &str) -> bool {
        self.turns.locked().contains_key(chat_id)
    }

    /// The pids of the agent processes that are up, replying or idle.
    pub fn agent_pids(&self) -> Vec<u32> {
        self.turns.locked().values().map(|turn| turn.pid).collect()
    }

    /// The chats with a reply under way, each with the folder it works in and what its files
    /// are judged by.
    pub(in crate::orchestration) fn attributions(
        &self,
    ) -> Vec<(String, String, SharedAttribution)> {
        let turns = self.turns.locked();
        turns
            .iter()
            .filter(|(_, turn)| !turn.idle.load(Ordering::SeqCst))
            .map(|(id, turn)| (id.clone(), turn.root.clone(), Arc::clone(&turn.attribution)))
            .collect()
    }

    /// The chats with a reply under way.
    pub fn running(&self) -> Vec<String> {
        let turns = self.turns.locked();
        turns
            .iter()
            .filter(|(_, turn)| !turn.idle.load(Ordering::SeqCst))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Stops every idle agent, as `retire_idle` does.
    pub fn retire_all_idle(&self) {
        for chat_id in self.chats() {
            self.retire_idle(&chat_id);
        }
    }

    /// Stops every idle agent now, as `replace_idle` does, even Antigravity at work by itself.
    pub(in crate::orchestration) fn stop_all_idle(&self) {
        for chat_id in self.chats() {
            self.replace_idle(&chat_id);
        }
    }

    /// The chats with an agent process up.
    fn chats(&self) -> Vec<String> {
        self.turns.locked().keys().cloned().collect()
    }

    /// Stops the chat's idle agent, and the shell commands it left running, so a turn that
    /// needs a process of its own can start. A turn that is replying, or Antigravity at work by
    /// itself, is left to finish, and is not used again once it is idle.
    pub fn retire_idle(&self, chat_id: &str) {
        self.stop_idle(chat_id, false);
    }

    /// Stops the chat's idle agent for a turn that starts now, even Antigravity at work by
    /// itself, which would otherwise run on beside the new process.
    pub(crate) fn replace_idle(&self, chat_id: &str) {
        self.stop_idle(chat_id, true);
    }

    /// Stops the chat's idle agent. Antigravity at work by itself is stopped only when
    /// `even_alone`; otherwise, as a replying agent, it is marked not to be used again.
    fn stop_idle(&self, chat_id: &str, even_alone: bool) {
        let mut turns = self.turns.locked();
        match turns.get(chat_id) {
            Some(turn)
                if turn.idle.load(Ordering::SeqCst)
                    && (even_alone || !turn.alone.load(Ordering::SeqCst)) => {}
            Some(turn) => {
                turn.stale.store(true, Ordering::SeqCst);
                return;
            }
            None => return,
        }
        let Some(turn) = turns.remove(chat_id) else {
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
    pub(in crate::orchestration) fn rebase_idle(&self, chat_id: &str) {
        let found = {
            let turns = self.turns.locked();
            turns
                .get(chat_id)
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
        let now = source_control::git_changes(&root).unwrap_or_default();
        let mut record = shared.locked();
        // Another message may have resumed it meanwhile, and that reply's record stays.
        if idle.load(Ordering::SeqCst) {
            record.restart(now);
        }
    }

    /// Hands a new compact window to the API turn replying in `chat_id`, which compacts by it
    /// from its next step. Other agents read it only when they start.
    pub fn set_compact_window(&self, chat_id: &str, window: u64) {
        let turns = self.turns.locked();
        if let Some(turn) = turns.get(chat_id)
            && turn.agent == provider::AgentKind::Api
            && !turn.idle.load(Ordering::SeqCst)
        {
            write_line(&turn.stdin, &provider::api::compact_window_line(window));
        }
    }

    /// Hands a message to the turn running in `chat_id`: written to the agent when it
    /// can take input mid-turn, otherwise kept to start the next turn.
    /// `subagents` are the subagent calls still running in the foreground.
    pub fn steer(
        &self,
        chat_id: &str,
        input: FollowUpInput<'_>,
        session_id: Option<&str>,
        subagents: &[String],
    ) -> Delivery {
        let FollowUpInput {
            message_id,
            text,
            attachments,
            images,
            merged,
        } = input;
        self.background_subagents(chat_id, subagents);
        let turns = self.turns.locked();
        let Some(turn) = turns.get(chat_id) else {
            return Delivery::NotRunning;
        };
        let follow_up = FollowUp {
            message_id: message_id.into(),
            text: text.into(),
            attachments: attachments.to_vec(),
            images: images.to_vec(),
            merged: merged.to_vec(),
        };
        self.handed_over.locked().insert(message_id.into());
        // A stopped turn takes nothing more; the message starts the next turn.
        if turn.cancelled.load(Ordering::SeqCst) {
            turn.queue.locked().push(follow_up);
            return Delivery::Queued;
        }
        let turn_id = turn.native.locked().clone();
        let prompt = provider::Prompt {
            text,
            attachments,
            images,
        };
        // Under the stdin lock, so the reader cannot close stdin between the write and
        // recording that an answer is due.
        let mut stdin = turn.stdin.locked();
        if turn.idle.load(Ordering::SeqCst) {
            // Antigravity at work by itself in a process that is not used again: the message
            // waits for that work to end, then starts a new process.
            if turn.stale.load(Ordering::SeqCst) && turn.alone.load(Ordering::SeqCst) {
                turn.queue.locked().push(follow_up);
                return Delivery::Queued;
            }
            let line = provider::resume_input(turn.agent, prompt, session_id, turn_id.as_deref());
            // The agent's last reply is over; this message starts the next one, and if the agent
            // can no longer take it, or was started with settings changed since, a new turn
            // replaces the idle process.
            if !turn.stale.load(Ordering::SeqCst)
                && line.is_some_and(|line| write_to(&mut stdin, &line))
            {
                turn.idle.store(false, Ordering::SeqCst);
                turn.steers.locked().push_back(follow_up);
                return Delivery::Resumed;
            }
            self.handed_over.locked().remove(message_id);
            return Delivery::NotRunning;
        }
        // Patchwork stopped the reply to compact it, and a stop now would stop the compaction:
        // the message goes once it ends, as a steer's would.
        if turn.compacting.load(Ordering::SeqCst) {
            turn.steers.locked().push_back(follow_up);
            return Delivery::Steered;
        }
        let line = provider::steer_input(turn.agent, prompt, session_id, turn_id.as_deref());
        if let Some(line) = line
            && write_to(&mut stdin, &line)
        {
            turn.steers.locked().push_back(follow_up);
            return Delivery::Steered;
        }
        turn.queue.locked().push(follow_up);
        Delivery::Queued
    }

    /// Stops the turn and drops the messages waiting for it. The agent's scope is stopped along
    /// with its process group, so a command it started in a session of its own, which
    /// could keep its output open, ends with it.
    pub fn cancel(&self, chat_id: &str) {
        let turns = self.turns.locked();
        if let Some(turn) = turns.get(chat_id) {
            stop_turn(turn);
            let (pid, exited, scope) = (turn.pid, Arc::clone(&turn.exited), turn.scope.clone());
            std::thread::spawn(move || {
                terminate(pid, exited);
                if let Some(unit) = scope {
                    scope::stop(&unit);
                }
            });
        } else {
            self.stop_when_started(chat_id);
        }
    }

    /// Stops the reply under way and leaves the messages waiting for it queued, as `cancel` does, but
    /// by interrupting the agent, which ends its turn cleanly in its session. Claude then stays
    /// up for the next message. An agent that cannot be interrupted, or that does not end its
    /// reply within `INTERRUPT_GRACE` or before Stop is pressed again, is stopped as `cancel`
    /// stops it. `session_id` is the
    /// chat's agent session, which Codex needs.
    pub fn interrupt(&self, chat_id: &str, session_id: Option<&str>) {
        let turns = self.turns.locked();
        let Some(turn) = turns.get(chat_id) else {
            self.stop_when_started(chat_id);
            return;
        };
        let turn_id = turn.native.locked().clone();
        // An idle agent has no reply to stop; it is stopped with the commands it left running.
        let line = (!turn.idle.load(Ordering::SeqCst))
            .then(|| provider::interrupt(turn.agent, session_id, turn_id.as_deref()))
            .flatten();
        // Stop pressed again while the agent has yet to end its reply ends it now.
        let Some(line) = line.filter(|_| !turn.interrupts.waiting()) else {
            drop(turns);
            return self.cancel(chat_id);
        };
        stop_turn(turn);
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

    /// How a turn that was stopped ends: interrupted when the app's quit stopped it, so it can
    /// resume at the next launch, and cancelled when the user did.
    pub(in crate::orchestration) fn stopped_status(&self) -> MessageStatus {
        if self.quitting.load(Ordering::SeqCst) {
            MessageStatus::Interrupted
        } else {
            MessageStatus::Cancelled
        }
    }

    /// Stops every running agent, so none outlives the app when it quits. Agents that
    /// are still running after `QUIT_GRACE` are sent SIGKILL.
    pub fn stop_all(&self) {
        self.quitting.store(true, Ordering::SeqCst);
        let running: Vec<(u32, Arc<AtomicBool>)> = self
            .turns
            .locked()
            .values()
            .map(|turn| {
                turn.cancelled.store(true, Ordering::SeqCst);
                (turn.pid, Arc::clone(&turn.exited))
            })
            .collect();
        for (pid, _) in &running {
            running::signal_process_group(*pid, running::SIGTERM);
        }
        let deadline = Instant::now() + QUIT_GRACE;
        while Instant::now() < deadline && running.iter().any(|(_, e)| !e.load(Ordering::SeqCst)) {
            // How often quitting checks whether every agent has exited, while it waits.
            std::thread::sleep(Duration::from_millis(50));
        }
        running
            .iter()
            .filter(|(_, exited)| !exited.load(Ordering::SeqCst))
            .for_each(|(pid, _)| running::signal_process_group(*pid, running::SIGKILL));
    }
}

/// Marks the turn stopped. The messages waiting for it stay queued, and the turn's end takes
/// them off its queue.
fn stop_turn(turn: &RunningTurn) {
    turn.cancelled.store(true, Ordering::SeqCst);
}
