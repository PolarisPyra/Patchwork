//! The `Runner`: the agent processes up in each chat, and handing a message to one, stopping
//! one, or stopping them all.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::chats::MessageStatus;
use crate::orchestration::changes::attribution::SharedAttribution;
use crate::orchestration::messages::queue::{FollowUp, FollowUpInput};
use crate::orchestration::turn::chat_run::ChatRun;
use crate::orchestration::turn::limit::LimitWait;
use crate::orchestration::turn::process::{
    INTERRUPT_GRACE, QUIT_GRACE, terminate, write_line, write_to,
};
use crate::orchestration::turn::start::{RunningTurn, Turn};
use crate::provider;
use crate::source_control;
use crate::system::processes::{running, scope};

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

/// The agent processes up in each chat, and what each chat's run holds.
///
/// # Locks
///
/// Everything the runner keeps of a chat is one `ChatRun` in `runs`, read and changed only
/// through `with_run`, `with_turn` and `with_runs`, so each change to a chat's run is one step
/// under one lock. Locks are taken in this order, never the other way:
///
/// 1. `runs`.
/// 2. A turn's own locks: `stdin`, then `queue`, `steers`, `pending`, `native`, `mode`,
///    `settings`, `controls` and `attribution`, each held on its own.
///
/// Under `runs` nothing takes the `Store`, publishes an event, starts a process or waits on
/// one. Writing a line to an agent's stdin is the one exception: a steer is written under
/// `runs` so the turn cannot end between the write and recording the steer. `limit_waits`
/// and `jobs` are separate locks that are never held with `runs`.
#[derive(Default)]
pub struct Runner {
    /// Each chat's run, by chat id. A chat with nothing running has no entry.
    runs: Mutex<HashMap<String, ChatRun>>,
    /// The commands Patchwork's own agent runs, which outlive its turns as background jobs.
    pub(in crate::orchestration) jobs: crate::orchestration::tasks::jobs::Jobs,
    /// The turns that resume when their agent's usage limit resets, by chat. Apart from `runs`
    /// as it outlives any turn and is saved to the store under its own lock.
    pub(in crate::orchestration) limit_waits: Mutex<HashMap<String, LimitWait>>,
    /// Set once the app is quitting. A turn stopped from then on was cut off by the quit, not
    /// stopped by the user, so it ends interrupted and resumes at the next launch.
    quitting: AtomicBool,
}

impl std::fmt::Debug for Runner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runner").finish_non_exhaustive()
    }
}

/// What `interrupt` found under the lock.
enum Interrupt {
    /// No agent is up; a start under way was asked to stop.
    NotUp,
    /// The agent is stopped as `cancel` stops it.
    Cancel,
    /// The interrupt was written, or not: the agent gets `wait` to end its reply.
    Sent {
        pid: u32,
        exited: Arc<AtomicBool>,
        scope: Option<String>,
        interrupts: Arc<crate::orchestration::turn::process::Interrupts>,
        asked: u64,
        wait: Duration,
    },
}

impl Runner {
    /// Runs `f` on the chat's run under the lock, and forgets the run once it holds nothing.
    pub(in crate::orchestration) fn with_run<R>(
        &self,
        chat_id: &str,
        f: impl FnOnce(&mut ChatRun) -> R,
    ) -> R {
        let mut runs = self.runs.locked();
        if !runs.contains_key(chat_id) {
            runs.insert(chat_id.to_owned(), ChatRun::default());
        }
        let run = runs.get_mut(chat_id).expect("the run was just inserted");
        let out = f(run);
        if run.is_empty() {
            runs.remove(chat_id);
        }
        out
    }

    /// Runs `f` on the agent up in the chat under the lock. None when no agent is up.
    pub(in crate::orchestration) fn with_turn<R>(
        &self,
        chat_id: &str,
        f: impl FnOnce(&RunningTurn) -> R,
    ) -> Option<R> {
        let runs = self.runs.locked();
        runs.get(chat_id).and_then(|run| run.turn.as_ref()).map(f)
    }

    /// Runs `f` on every chat's run under the lock, and forgets the runs that hold nothing.
    pub(in crate::orchestration) fn with_runs<R>(
        &self,
        f: impl FnOnce(&mut HashMap<String, ChatRun>) -> R,
    ) -> R {
        let mut runs = self.runs.locked();
        let out = f(&mut runs);
        runs.retain(|_, run| !run.is_empty());
        out
    }

    /// The agents that are up, each with its chat.
    fn turns<R>(
        &self,
        f: impl FnOnce(&mut dyn Iterator<Item = (&String, &RunningTurn)>) -> R,
    ) -> R {
        let runs = self.runs.locked();
        let mut turns = runs
            .iter()
            .filter_map(|(id, run)| run.turn.as_ref().map(|turn| (id, turn)));
        f(&mut turns)
    }

    /// Whether a turn is being started in the chat, before its agent is up.
    pub fn is_starting(&self, chat_id: &str) -> bool {
        self.runs
            .locked()
            .get(chat_id)
            .is_some_and(|run| run.starting)
    }

    /// Records the agent `pid` that `turn` started. True when the chat was stopped while the
    /// turn was starting, so the caller stops the agent now that it is up.
    pub(in crate::orchestration) fn register(&self, turn: &Turn, pid: u32) -> bool {
        self.with_run(&turn.chat_id, |run| {
            run.turn = Some(turn.running(pid));
            std::mem::take(&mut run.stop_when_up)
        })
    }

    /// How many chats the runner keeps a run for.
    #[cfg(test)]
    pub(in crate::orchestration) fn tracked(&self) -> usize {
        self.runs.locked().len()
    }

    /// Whether a reply is under way in the chat. An agent that is only kept up by shell
    /// commands it left running does not count.
    pub fn is_running(&self, chat_id: &str) -> bool {
        self.with_turn(chat_id, |turn| !turn.idle.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    /// Whether an agent process is up in the chat, replying or idle.
    pub fn has_agent(&self, chat_id: &str) -> bool {
        self.with_turn(chat_id, |_| ()).is_some()
    }

    /// The pids of the agent processes that are up, replying or idle.
    pub fn agent_pids(&self) -> Vec<u32> {
        self.turns(|turns| turns.map(|(_, turn)| turn.pid).collect())
    }

    /// The chats with a reply under way, each with the folder it works in and what its files
    /// are judged by.
    pub(in crate::orchestration) fn attributions(
        &self,
    ) -> Vec<(String, String, SharedAttribution)> {
        self.turns(|turns| {
            turns
                .filter(|(_, turn)| !turn.idle.load(Ordering::SeqCst))
                .map(|(id, turn)| (id.clone(), turn.root.clone(), Arc::clone(&turn.attribution)))
                .collect()
        })
    }

    /// The chats with a reply under way.
    pub fn running(&self) -> Vec<String> {
        self.turns(|turns| {
            turns
                .filter(|(_, turn)| !turn.idle.load(Ordering::SeqCst))
                .map(|(id, _)| id.clone())
                .collect()
        })
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
        self.turns(|turns| turns.map(|(id, _)| id.clone()).collect())
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
        let stopped = self.with_run(chat_id, |run| {
            match &run.turn {
                Some(turn)
                    if turn.idle.load(Ordering::SeqCst)
                        && (even_alone || !turn.alone.load(Ordering::SeqCst)) => {}
                Some(turn) => {
                    turn.stale.store(true, Ordering::SeqCst);
                    return None;
                }
                None => return None,
            }
            run.turn.take()
        });
        let Some(turn) = stopped else {
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
        let found = self
            .with_turn(chat_id, |turn| {
                turn.idle.load(Ordering::SeqCst).then(|| {
                    (
                        turn.root.clone(),
                        Arc::clone(&turn.idle),
                        Arc::clone(&turn.attribution),
                    )
                })
            })
            .flatten();
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

    /// Hands a new compact window to the turn in `chat_id` so it compacts by it from its next step.
    pub fn set_compact_window(&self, chat_id: &str, window: u64) {
        self.with_turn(chat_id, |turn| {
            turn.compact_window.store(window, Ordering::SeqCst);
            if turn.agent == provider::AgentKind::Api && !turn.idle.load(Ordering::SeqCst) {
                write_line(&turn.stdin, &provider::api::compact_window_line(window));
            }
        });
    }

    /// Hands the chat's new model and effort to its Antigravity agent, which takes them at its
    /// next reply. False when the chat's agent needs a new process for them.
    pub(crate) fn set_choice(
        &self,
        chat_id: &str,
        agent: provider::AgentKind,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> bool {
        self.with_turn(chat_id, |turn| {
            if turn.agent != provider::AgentKind::Antigravity || agent != turn.agent {
                return false;
            }
            let mut settings = turn.settings.locked();
            // agy has no "back to your default", so going back names its default outright; with
            // no probe yet to say what that is, a new process starts on it.
            let model_back = model.is_none() && settings.model.is_some();
            let effort_back = effort.is_none() && settings.effort.is_some();
            let (default_model, default_effort) = if model_back || effort_back {
                match provider::antigravity::known_defaults() {
                    Some((model, effort)) => (Some(model), effort),
                    None => return false,
                }
            } else {
                (None, None)
            };
            if effort_back && default_effort.is_none() {
                return false;
            }
            settings.model = match model {
                Some(model) => Some(model.to_owned()),
                None if model_back => default_model,
                None => None,
            };
            settings.effort = match effort {
                Some(effort) => Some(effort.to_owned()),
                None if effort_back => default_effort,
                None => None,
            };
            true
        })
        .unwrap_or(false)
    }

    /// Whether the running turn in `chat_id` is currently compacting mid-turn.
    pub(crate) fn is_compacting(&self, chat_id: &str) -> bool {
        self.with_turn(chat_id, |turn| turn.compacting.load(Ordering::SeqCst))
            .unwrap_or(false)
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
        self.with_run(chat_id, |run| {
            let Some(turn) = &run.turn else {
                return Delivery::NotRunning;
            };
            let follow_up = FollowUp {
                message_id: message_id.into(),
                text: text.into(),
                attachments: attachments.to_vec(),
                images: images.to_vec(),
                merged: merged.to_vec(),
            };
            run.handed_over.insert(message_id.into());
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
                // Antigravity at work by itself in a process that is not used again: the
                // message waits for that work to end, then starts a new process.
                if turn.stale.load(Ordering::SeqCst) && turn.alone.load(Ordering::SeqCst) {
                    turn.queue.locked().push(follow_up);
                    return Delivery::Queued;
                }
                let line =
                    provider::resume_input(turn.agent, prompt, session_id, turn_id.as_deref()).map(
                        |line| {
                            let mode = *turn.mode.locked();
                            turn.settings
                                .locked()
                                .first_line(turn.agent, mode, session_id, line)
                        },
                    );
                // The agent's last reply is over; this message starts the next one, and if the
                // agent can no longer take it, or was started with settings changed since, a new
                // turn replaces the idle process.
                if !turn.stale.load(Ordering::SeqCst)
                    && line.is_some_and(|line| write_to(&mut stdin, &line))
                {
                    turn.idle.store(false, Ordering::SeqCst);
                    turn.steers.locked().push_back(follow_up);
                    turn.wake.reply_due();
                    return Delivery::Resumed;
                }
                drop(stdin);
                run.handed_over.remove(message_id);
                return Delivery::NotRunning;
            }
            // Patchwork stopped the reply to compact it, and a stop now would stop the
            // compaction: the message goes once it ends, as a steer's would.
            if turn.compacting.load(Ordering::SeqCst) {
                turn.steers.locked().push_back(follow_up);
                return Delivery::Steered;
            }
            // Antigravity steers by cancelling the in-flight prompt and sending all accumulated
            // steers once it settles; a second `session/cancel` would race with the steered
            // prompt. A message that only resumed the idle agent waits there too, but cancels
            // nothing.
            if turn.agent == provider::AgentKind::Antigravity
                && turn.cancelling.load(Ordering::SeqCst)
            {
                turn.steers.locked().push_back(follow_up);
                return Delivery::Steered;
            }
            let line = provider::steer_input(turn.agent, prompt, session_id, turn_id.as_deref());
            if let Some(line) = line
                && write_to(&mut stdin, &line)
            {
                let id = follow_up.message_id.clone();
                turn.steers.locked().push_back(follow_up);
                drop(stdin);
                watch_steer(turn, &id, line);
                return Delivery::Steered;
            }
            turn.queue.locked().push(follow_up);
            Delivery::Queued
        })
    }

    /// Stops the turn and drops the messages waiting for it. The agent's scope is stopped along
    /// with its process group, so a command it started in a session of its own, which
    /// could keep its output open, ends with it.
    pub fn cancel(&self, chat_id: &str) {
        let stopped = self.with_run(chat_id, |run| {
            let Some(turn) = &run.turn else {
                // Not up yet: the start stops it once it is.
                if run.starting {
                    run.stop_when_up = true;
                }
                return None;
            };
            stop_turn(turn);
            Some((turn.pid, Arc::clone(&turn.exited), turn.scope.clone()))
        });
        let Some((pid, exited, scope)) = stopped else {
            return;
        };
        std::thread::spawn(move || {
            terminate(pid, exited);
            if let Some(unit) = scope {
                scope::stop(&unit);
            }
        });
    }

    /// Stops the reply under way and leaves the messages waiting for it queued, as `cancel` does, but
    /// by interrupting the agent, which ends its turn cleanly in its session. Claude then stays
    /// up for the next message. An agent that cannot be interrupted, or that does not end its
    /// reply within `INTERRUPT_GRACE` or before Stop is pressed again, is stopped as `cancel`
    /// stops it. `session_id` is the
    /// chat's agent session, which Codex needs.
    pub fn interrupt(&self, chat_id: &str, session_id: Option<&str>) {
        let found = self.with_run(chat_id, |run| {
            let Some(turn) = &run.turn else {
                if run.starting {
                    run.stop_when_up = true;
                }
                return Interrupt::NotUp;
            };
            let turn_id = turn.native.locked().clone();
            // An idle agent has no reply to stop; it is stopped with the commands it left
            // running.
            let line = (!turn.idle.load(Ordering::SeqCst))
                .then(|| provider::interrupt(turn.agent, session_id, turn_id.as_deref()))
                .flatten();
            // Stop pressed again while the agent has yet to end its reply ends it now.
            let Some(line) = line.filter(|_| !turn.interrupts.waiting()) else {
                return Interrupt::Cancel;
            };
            stop_turn(turn);
            let asked = turn.interrupts.ask();
            let wait = if write_line(&turn.stdin, &line) {
                INTERRUPT_GRACE
            } else {
                Duration::ZERO
            };
            Interrupt::Sent {
                pid: turn.pid,
                exited: Arc::clone(&turn.exited),
                scope: turn.scope.clone(),
                interrupts: Arc::clone(&turn.interrupts),
                asked,
                wait,
            }
        });
        let Interrupt::Sent {
            pid,
            exited,
            scope,
            interrupts,
            asked,
            wait,
        } = found
        else {
            if matches!(found, Interrupt::Cancel) {
                self.cancel(chat_id);
            }
            return;
        };
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
        let running: Vec<(u32, Arc<AtomicBool>)> = self.turns(|turns| {
            turns
                .map(|(_, turn)| {
                    turn.cancelled.store(true, Ordering::SeqCst);
                    (turn.pid, Arc::clone(&turn.exited))
                })
                .collect()
        });
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

/// How long Antigravity gets to stop its reply for a steer before the stop is written again:
/// agy drops a `session/cancel` that arrives before it has taken the prompt in.
const STEER_RETRY: Duration = Duration::from_secs(3);

/// How long Antigravity gets to stop its reply for a steer before its process is stopped. The
/// turn's end then sends the steer in a new turn, so it is never left waiting.
const STEER_GRACE: Duration = Duration::from_secs(30);

/// How often a steer's watch checks whether Antigravity answered the stop.
const STEER_POLL: Duration = Duration::from_millis(100);

/// Watches the stop `cancel` that Antigravity was sent for the steer `message_id` until the
/// reply ends and the steer goes out, writing it again once and stopping the process past
/// `STEER_GRACE`. Other agents take steers without stopping, so they are not watched.
pub(in crate::orchestration) fn watch_steer(turn: &RunningTurn, message_id: &str, cancel: String) {
    if turn.agent != provider::AgentKind::Antigravity {
        return;
    }
    turn.cancelling.store(true, Ordering::SeqCst);
    let (steers, stdin, exited) = (
        Arc::clone(&turn.steers),
        Arc::clone(&turn.stdin),
        Arc::clone(&turn.exited),
    );
    let (pid, scope, message_id) = (turn.pid, turn.scope.clone(), message_id.to_string());
    std::thread::spawn(move || {
        let waiting = || {
            !exited.load(Ordering::SeqCst)
                && steers.locked().iter().any(|f| f.message_id == message_id)
        };
        let started = Instant::now();
        let mut retried = false;
        while waiting() {
            let waited = started.elapsed();
            if waited >= STEER_GRACE {
                log::warn!("Antigravity did not stop its reply for a steer; stopping its process");
                terminate(pid, exited);
                if let Some(unit) = scope {
                    scope::stop(&unit);
                }
                return;
            }
            if !retried && waited >= STEER_RETRY {
                retried = true;
                // Under the stdin lock, where the steer is sent once the reply ends, so the
                // repeated stop cannot land on the steer's own prompt.
                let mut stdin = stdin.locked();
                if waiting() {
                    write_to(&mut stdin, &cancel);
                }
            }
            std::thread::sleep(STEER_POLL);
        }
    });
}

/// Marks the turn stopped. The messages waiting for it stay queued, and the turn's end takes
/// them off its queue.
fn stop_turn(turn: &RunningTurn) {
    turn.cancelled.store(true, Ordering::SeqCst);
}
