//! Antigravity at work with no prompt open. Once a command it moved to the background ends, it
//! goes on by itself, and nothing marks when it stops; its output is read with a timeout, so
//! the reply it started ends once it goes quiet. The wait also gives way to a running swarm,
//! whose files are reread on a timer.

// Rust guideline compliant 2026-10-09

use std::cell::Cell;
use std::collections::HashSet;
use std::io::BufReader;
use std::process::ChildStdout;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::events::ChatEvent;
use crate::orchestration::turn::process::next_line;
use crate::system::Locked;

/// How long Antigravity says nothing, with no call open, before a reply it started by itself
/// ends. Between steps it has been seen thinking silently for up to 29 s.
const QUIET_END: Duration = Duration::from_secs(45);

/// The agent's stdout, read on a thread of its own so that waiting for a line can time out.
#[derive(Debug)]
pub(super) struct Lines {
    received: Receiver<Input>,
    /// When the reader last got something, which the agent's silence is counted from.
    last: Cell<Instant>,
}

/// What reaches the reader of the agent's output.
#[derive(Debug)]
enum Input {
    Line(String),
    /// The idle agent was sent a message, which `Wake` reports.
    Woken,
}

/// Wakes the reader of the agent's output while that output is open, so the reply to a
/// message the idle agent was sent starts before the agent says anything. Antigravity says
/// nothing at all while it runs `/compact`.
#[derive(Debug, Clone, Default)]
pub(in crate::orchestration) struct Wake(Arc<Mutex<Option<Sender<Input>>>>);

impl Wake {
    /// Has the reader start the reply to the message the idle agent was just sent.
    pub(in crate::orchestration) fn reply_due(&self) {
        if let Some(lines) = self.0.locked().as_ref() {
            // Fails only once the output has closed, when there is no reply to start.
            let _ = lines.send(Input::Woken);
        }
    }
}

/// What waiting for the agent's next line found.
#[derive(Debug)]
pub(super) enum Next {
    Line(String),
    /// The idle agent was sent a message; its reply starts now.
    Woken,
    /// The agent said nothing for `QUIET_END`.
    Quiet,
    /// The `tick` the reader asked for came first.
    Tick,
    /// The output has ended or cannot be read.
    Closed,
}

impl Lines {
    /// Starts reading `stdout` line by line until it closes; `wake` reaches the reader until then.
    pub(super) fn read(stdout: ChildStdout, wake: &Wake) -> Self {
        let (lines, received) = mpsc::channel();
        *wake.0.locked() = Some(lines.clone());
        let wake = wake.clone();
        std::thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            while let Some(line) = next_line(&mut stdout) {
                if lines.send(Input::Line(line)).is_err() {
                    break;
                }
            }
            // So the reader finds the output closed once this thread's sender goes.
            wake.0.locked().take();
        });
        Self {
            received,
            last: Cell::new(Instant::now()),
        }
    }

    /// The next line. While `unprompted` waits for the agent to go quiet, `Quiet` once it has,
    /// and `Tick` once `tick` passes with nothing read.
    pub(super) fn next(&self, unprompted: &Unprompted, tick: Option<Duration>) -> Next {
        let quiet = unprompted
            .waits_for_quiet()
            .then(|| QUIET_END.saturating_sub(self.last.get().elapsed()));
        let wait = match (quiet, tick) {
            (Some(quiet), Some(tick)) => Some(quiet.min(tick)),
            (quiet, tick) => quiet.or(tick),
        };
        let input = if let Some(wait) = wait {
            match self.received.recv_timeout(wait) {
                Ok(input) => input,
                Err(RecvTimeoutError::Timeout) if quiet.is_some_and(|quiet| quiet <= wait) => {
                    return Next::Quiet;
                }
                Err(RecvTimeoutError::Timeout) => return Next::Tick,
                Err(RecvTimeoutError::Disconnected) => return Next::Closed,
            }
        } else {
            match self.received.recv() {
                Ok(input) => input,
                Err(_) => return Next::Closed,
            }
        };
        self.last.set(Instant::now());
        match input {
            Input::Line(line) => Next::Line(line),
            Input::Woken => Next::Woken,
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
    pub(super) fn track(&mut self, event: &ChatEvent) {
        let Self::Working(running) = self else {
            return;
        };
        match event {
            ChatEvent::ToolStarted { tool, .. } => {
                running.insert(tool.id.clone());
            }
            ChatEvent::ToolFinished { tool_id, .. } => {
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
