//! Runs agent turns by spawning the agent CLIs and translating their JSONL output into
//! `AgentEvent`s. Each turn is one child process; its stdin stays open while the turn runs so
//! approvals can be answered. `Runner` is the entry point; the other modules each cover one part
//! of a turn, and `claude` and `codex` hold the instructions each agent is launched with.

pub mod checkpoint;
pub mod claude;
pub mod codex;
mod attribution;
mod ingest;
pub(crate) mod goal;
mod process;
mod queue;
pub(crate) mod recovery;
mod stream;
mod stuck;
mod subagents;
mod turn;
mod usage;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use process::{QUIT_GRACE, terminate, write_line, write_to};
use queue::FollowUp;
pub use queue::FollowUpInput;
pub use turn::TurnRequest;

/// The user's answer to an approval request: whether it was allowed, any answers to questions,
/// and the rule the user saved with it.
pub struct ApprovalDecision<'a> {
    pub approval_id: &'a str,
    pub allow: bool,
    pub answers: HashMap<String, Vec<String>>,
    pub rule: Option<RuleRef>,
}
use turn::RunningTurn;

pub(crate) use stream::publish;
use crate::permissions::{self, Decision, Target};
use crate::provider;
use crate::scope;
use crate::Server;
use crate::types::{AgentKind, Settings, AgentEvent, RuleRef};
use crate::workspace;

/// What the permission rules answer to `target` in the thread, or None to ask the user.
fn decide(server: &Server, thread_id: &str, root: &str, target: &Target) -> Option<Decision> {
    let project = server.store.thread(thread_id).and_then(|t| t.project_id);
    permissions::decide(
        &server.store.settings().permissions,
        project.as_deref(),
        std::path::Path::new(root),
        target,
    )
}

/// Whether `text` is upkeep the app also sends on its own rather than work for the agent: an idle
/// `/compact`, or the message that has the agent drop its goal.
pub fn is_upkeep(text: &str) -> bool {
    let text = text.trim();
    text == "/compact"
        || [AgentKind::Claude, AgentKind::Antigravity]
            .into_iter()
            .any(|agent| text == goal::clear_command(agent))
}

/// Which shells are installed and which one runs the agent's commands, for agents that take
/// instructions at launch.
fn shell_instructions(settings: &Settings) -> String {
    let mut prompt = String::new();
    let installed = crate::available_shells();
    if !installed.is_empty() {
        prompt.push_str(&format!(
            "Shells on this machine: {}.",
            installed.join(", ")
        ));
    }
    let chosen = settings.shell.as_deref();
    if let Some(shell) = crate::effective_shell(chosen) {
        if chosen == Some(shell.as_str()) {
            prompt.push_str(&format!(
                " The user chose {shell} for shell commands; run them in it and write them for it."
            ));
        } else {
            prompt.push_str(&format!(
                " Shell commands run in {shell}, the user's login shell; write them for it."
            ));
        }
    }
    prompt.trim_start().to_string()
}

/// What became of a message sent to a thread.
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
    turns: Mutex<HashMap<String, RunningTurn>>,
    /// Messages this process has handed to a turn, whether steered or queued. Written under the
    /// `turns` lock, so a withdrawal sees it in the same order as the queue it checks.
    handed_over: Mutex<HashSet<String>>,
}

impl Runner {
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
    fn attributions(&self) -> Vec<(String, attribution::SharedAttribution)> {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        turns
            .iter()
            .filter(|(_, turn)| !turn.idle.load(Ordering::SeqCst))
            .map(|(id, turn)| (id.clone(), turn.attribution.clone()))
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

    /// Stops the thread's idle agent, and the shell commands it left running, so a turn that
    /// needs a process of its own can start. A turn that is replying is left alone.
    pub fn retire_idle(&self, thread_id: &str) {
        let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        if !turns
            .get(thread_id)
            .is_some_and(|turn| turn.idle.load(Ordering::SeqCst))
        {
            return;
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
        let FollowUpInput { message_id, text, attachments, images } = input;
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
            .native_turn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let prompt = provider::Prompt { text, attachments, images };
        let line = provider::steer_input(turn.agent, prompt, session_id, turn_id.as_deref());
        // Under the stdin lock, so the reader cannot close stdin between the write and
        // recording that an answer is due.
        let mut stdin = turn.stdin.lock().unwrap_or_else(PoisonError::into_inner);
        if turn.idle.load(Ordering::SeqCst) {
            // The agent's last reply is over; this message starts the next one, and if the agent
            // can no longer take it, a new turn replaces the idle process.
            if line.is_some_and(|line| write_to(&mut stdin, &line)) {
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
    pub fn cancel(&self, thread_id: &str, last_line: Option<String>) -> Vec<String> {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        let mut dropped = Vec::new();
        if let Some(turn) = turns.get(thread_id) {
            turn.cancelled.store(true, Ordering::SeqCst);
            dropped = turn
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .drain(..)
                .map(|f| f.message_id)
                .collect();
            let (pid, exited, scope) = (turn.pid, turn.exited.clone(), turn.scope.clone());
            let grace = if last_line.is_some_and(|line| write_line(&turn.stdin, &line)) {
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
        }
        dropped
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
                (turn.pid, turn.exited.clone())
            })
            .collect();
        running.iter().for_each(|(pid, _)| workspace::signal_process_group(*pid, workspace::SIGTERM));
        let deadline = Instant::now() + QUIT_GRACE;
        while Instant::now() < deadline && running.iter().any(|(_, e)| !e.load(Ordering::SeqCst)) {
            std::thread::sleep(Duration::from_millis(50));
        }
        running
            .iter()
            .filter(|(_, exited)| !exited.load(Ordering::SeqCst))
            .for_each(|(pid, _)| workspace::signal_process_group(*pid, workspace::SIGKILL));
    }

    /// Sends the user's answer to a pending approval and records it.
    pub fn respond(
        &self,
        server: &Server,
        thread_id: &str,
        decision: ApprovalDecision<'_>,
    ) -> Result<(), String> {
        self.resolve(server, thread_id, decision, false)
    }

    /// Answers the pending approvals the permission rules now cover, in every running turn, once
    /// the user changed the rules.
    pub fn apply_rules(&self, server: &Server) {
        let waiting: Vec<(String, String, Target, String)> = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            turns
                .iter()
                .flat_map(|(thread_id, turn)| {
                    let pending = turn.pending.lock().unwrap_or_else(PoisonError::into_inner);
                    pending
                        .iter()
                        .filter_map(|(id, p)| {
                            let target = p.target.clone()?;
                            Some((thread_id.clone(), id.clone(), target, p.root.clone()))
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        for (thread_id, approval_id, target, root) in waiting {
            let Some(Decision { allow, rule }) = decide(server, &thread_id, &root, &target) else {
                continue;
            };
            let decision = ApprovalDecision {
                approval_id: &approval_id,
                allow,
                answers: HashMap::new(),
                rule: Some(rule),
            };
            // The user may have answered it meanwhile.
            let _ = self.resolve(server, &thread_id, decision, true);
        }
    }

    /// Sends an answer to a pending approval and records it; `auto` when a rule gave it.
    fn resolve(
        &self,
        server: &Server,
        thread_id: &str,
        decision: ApprovalDecision<'_>,
        auto: bool,
    ) -> Result<(), String> {
        let ApprovalDecision { approval_id, allow, answers, rule } = decision;
        let (agent, stdin, pending) = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            let turn = turns
                .get(thread_id)
                .ok_or("This turn is no longer running")?;
            let pending = turn
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(approval_id)
                .ok_or("This request was already answered")?;
            (turn.agent, turn.stdin.clone(), pending)
        };
        write_line(
            &stdin,
            &provider::approval_reply(agent, &pending.reply, allow, &answers),
        );
        publish(
            server,
            thread_id,
            AgentEvent::ApprovalResolved {
                message_id: pending.message_id,
                approval_id: approval_id.into(),
                allowed: allow,
                answers,
                rule,
                auto,
            },
        );
        Ok(())
    }
}
