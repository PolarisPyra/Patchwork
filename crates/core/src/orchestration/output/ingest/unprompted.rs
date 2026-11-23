//! Antigravity at work with no prompt open. Once a command it moved to the background ends, it
//! goes on by itself, and nothing marks when it stops; its output is read with a timeout, so
//! the reply it started ends once it goes quiet.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::io::BufReader;
use std::process::ChildStdout;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use crate::events::AgentEvent;
use crate::orchestration::turn::process::next_line;

/// How long Antigravity says nothing, with no call open, before a reply it started by itself
/// ends. Between steps it has been seen thinking silently for up to 29 s.
const QUIET_END: Duration = Duration::from_secs(45);

/// The agent's stdout, read on a thread of its own so that waiting for a line can time out.
#[derive(Debug)]
pub(super) struct Lines(Receiver<String>);

/// What waiting for the agent's next line found.
#[derive(Debug)]
pub(super) enum Next {
    Line(String),
    /// The agent said nothing for `QUIET_END`.
    Quiet,
    /// The output has ended or cannot be read.
    Closed,
}

impl Lines {
    /// Starts reading `stdout` line by line until it closes.
    pub(super) fn read(stdout: ChildStdout) -> Self {
        let (lines, received) = mpsc::channel();
        std::thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            while let Some(line) = next_line(&mut stdout) {
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
        Self(received)
    }

    /// The next line. While `unprompted` waits for the agent to go quiet, `Quiet` once it has.
    pub(super) fn next(&self, unprompted: &Unprompted) -> Next {
        if !unprompted.waits_for_quiet() {
            return self.0.recv().map_or(Next::Closed, Next::Line);
        }
        match self.0.recv_timeout(QUIET_END) {
            Ok(line) => Next::Line(line),
            Err(RecvTimeoutError::Timeout) => Next::Quiet,
            Err(RecvTimeoutError::Disconnected) => Next::Closed,
        }
    }
}

/// Whether Antigravity is at work with no prompt open.
#[derive(Debug, Default)]
pub(super) enum Unprompted {
    /// A prompt is open, or the agent is quiet.
    #[default]
    No,
    /// It went on by itself after answering its prompt. Holds the ids of its calls still running.
    Working(HashSet<String>),
}

impl Unprompted {
    pub(super) fn is_working(&self) -> bool {
        matches!(self, Self::Working(_))
    }

    /// Moves to `next`, and tells the runner through `alone` whether the agent works by itself.
    pub(super) fn set(&mut self, next: Self, alone: &AtomicBool) {
        alone.store(next.is_working(), Ordering::SeqCst);
        *self = next;
    }

    /// Notes a call starting or finishing while the agent works by itself.
    pub(super) fn track(&mut self, event: &AgentEvent) {
        let Self::Working(running) = self else {
            return;
        };
        match event {
            AgentEvent::ToolStarted { tool, .. } => {
                running.insert(tool.id.clone());
            }
            AgentEvent::ToolFinished { tool_id, .. } => {
                running.remove(tool_id);
            }
            _ => {}
        }
    }

    /// A long call says nothing while it runs, so only silence with no call open ends the reply.
    fn waits_for_quiet(&self) -> bool {
        matches!(self, Self::Working(running) if running.is_empty())
    }
}
