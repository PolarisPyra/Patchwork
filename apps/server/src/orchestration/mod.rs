//! Runs agent turns by spawning the agent CLIs and translating their JSONL output into
//! `AgentEvent`s. Each turn is one child process; its stdin stays open while the turn runs so
//! approvals can be answered. `Runner` is the entry point; the other modules each cover one part
//! of a turn, and `claude` and `codex` hold the instructions each agent is launched with.

pub mod checkpoint;
pub mod claude;
pub mod codex;
mod ingest;
pub(crate) mod goal;
mod process;
mod queue;
pub(crate) mod recovery;
mod stream;
mod subagents;
mod turn;
mod usage;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::provider::{self, Parsed, TurnSpec};
use crate::scope;
use crate::types::*;
use crate::workspace;
use crate::{Res, Server};
use serde_json::Value;

use ingest::*;
use process::*;
use queue::*;
use stream::*;
use subagents::*;
use turn::*;
use usage::*;
use goal::end_input;

pub(crate) use stream::publish;

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
    pub fn is_running(&self, thread_id: &str) -> bool {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(thread_id)
    }

    /// The threads with a turn running.
    pub fn running(&self) -> Vec<String> {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        turns.keys().cloned().collect()
    }


    /// Hands a message to the turn running in `thread_id`: written to the agent when it
    /// can take input mid-turn, otherwise kept to start the next turn.
    /// `subagents` are the subagent calls still running in the foreground.
    #[allow(clippy::too_many_arguments)]
    pub fn steer(
        &self,
        thread_id: &str,
        message_id: &str,
        session_id: Option<&str>,
        text: &str,
        attachments: &[Attachment],
        images: &[provider::Image],
        subagents: &[String],
    ) -> Delivery {
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
        let line = provider::steer_input(
            turn.agent,
            text,
            attachments,
            images,
            session_id,
            turn_id.as_deref(),
        );
        // Under the stdin lock, so the reader cannot close stdin between the write and
        // recording that an answer is due.
        let mut stdin = turn.stdin.lock().unwrap_or_else(PoisonError::into_inner);
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
        for (pid, _) in &running {
            workspace::signal_process_group(*pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + QUIT_GRACE;
        while Instant::now() < deadline && running.iter().any(|(_, e)| !e.load(Ordering::SeqCst)) {
            std::thread::sleep(Duration::from_millis(50));
        }
        for (pid, exited) in &running {
            if !exited.load(Ordering::SeqCst) {
                workspace::signal_process_group(*pid, libc::SIGKILL);
            }
        }
    }

    /// Sends the user's answer to a pending approval and records it.
    pub fn respond(
        &self,
        server: &Server,
        thread_id: &str,
        approval_id: &str,
        allow: bool,
        answers: HashMap<String, Vec<String>>,
    ) -> Result<(), String> {
        let (agent, stdin, (message_id, reply)) = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            let turn = turns
                .get(thread_id)
                .ok_or("This turn is no longer running")?;
            let reply = turn
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(approval_id)
                .ok_or("This request was already answered")?;
            (turn.agent, turn.stdin.clone(), reply)
        };
        write_line(
            &stdin,
            &provider::approval_reply(agent, &reply, allow, &answers),
        );
        publish(
            server,
            thread_id,
            AgentEvent::ApprovalResolved {
                message_id: message_id.clone(),
                approval_id: approval_id.into(),
                allowed: allow,
                answers,
            },
        );
        Ok(())
    }
}
