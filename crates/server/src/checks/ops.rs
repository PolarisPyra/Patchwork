//! Running the check command for a reply: who may start a run, the run itself, and what the
//! reply's card shows while it runs and once it ends.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::orchestration::{publish, AgentEvent};
use crate::persistence::Store;
use crate::system::now_ms;
use crate::threads::{CheckStatus, MessageStatus, TurnChecks};
use crate::Server;

use super::{KEEP, LIMIT, command, failures, run, tests_ran, Command};

/// Most failures listed for one run.
const MOST: usize = 12;

impl Server {
    /// Runs the project's check command for the reply `message_id` of `thread_id`, on a thread of
    /// its own, publishing each change as it happens. The person's explicit request, so it runs
    /// even for a project whose checks are otherwise off.
    ///
    /// # Errors
    ///
    /// Returns an error when the checks are already running for this reply.
    pub fn run_checks(self: &Arc<Self>, thread_id: &str, message_id: &str) -> Result<(), Error> {
        let claim = claim(message_id).ok_or("The checks are still running for this reply.")?;
        let (server, thread, message) = (
            Arc::clone(self),
            thread_id.to_string(),
            message_id.to_string(),
        );
        std::thread::spawn(move || {
            check_turn(&server, &thread, &message, true);
            drop(claim);
        });
        Ok(())
    }

    /// Runs the checks after a turn, unless the project's checks are off or it has nothing to run.
    /// Called once the turn's reply is saved, off the turn's thread.
    pub fn check_after_turn(self: &Arc<Self>, thread_id: &str, message_id: &str) {
        // Read here as well as in `check_project`, so a project with its checks off costs no
        // thread at all, which is every project until one is turned on.
        if let Some(project_id) = project_of(self, thread_id)
            && !enabled(&self.store, &project_id)
        {
            return;
        }
        let Some(claim) = claim(message_id) else {
            return;
        };
        let (server, thread, message) = (
            Arc::clone(self),
            thread_id.to_string(),
            message_id.to_string(),
        );
        std::thread::spawn(move || {
            check_turn(&server, &thread, &message, false);
            drop(claim);
        });
    }
}

/// The replies whose checks are running. Two runs for one reply would race each other for its
/// card, and the one that finished first could be what it is left showing.
static RUNNING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A reply's place in [`RUNNING`], given up when it is dropped.
struct Claim(String);

impl Drop for Claim {
    fn drop(&mut self) {
        RUNNING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|id| *id != self.0);
    }
}

/// Claims `message_id` for one run of its checks, or None while another run has it.
fn claim(message_id: &str) -> Option<Claim> {
    let mut running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
    if running.iter().any(|id| id == message_id) {
        return None;
    }
    running.push(message_id.to_string());
    Some(Claim(message_id.to_string()))
}

/// Runs the project's check command for the reply `message_id` of `thread_id`, publishing the
/// result as it starts and again when it ends. Blocks for as long as the command runs, so it runs
/// on a thread of its own. `asked` is a person's explicit request: it runs a project's checks even
/// when they are off.
fn check_turn(server: &Arc<Server>, thread_id: &str, message_id: &str, asked: bool) {
    let Some(project_id) = project_of(server, thread_id) else {
        return;
    };
    let Some(root) = server.project_root(&project_id).ok() else {
        return;
    };
    let emit = |checks: TurnChecks| {
        publish(
            server,
            thread_id,
            AgentEvent::ChecksRecorded {
                message_id: message_id.to_string(),
                checks,
            },
        );
    };
    check_project(&server.store, &project_id, &root, asked, now_ms(), &emit);
}

/// Checks the project at `root` and hands each card it comes to, in order, to `emit`: the running
/// one, then the result. Says nothing at all when the project has nothing to run, so a reply in a
/// folder without tests shows no checks rather than one that says it cannot be checked.
pub(super) fn check_project(
    store: &Store,
    project_id: &str,
    root: &str,
    asked: bool,
    at: u64,
    emit: &dyn Fn(TurnChecks),
) {
    let enabled = enabled(store, project_id);
    // Off, a reply says nothing about checks rather than carrying a card that says it was not
    // checked: the switch lives in Settings, and a card in every reply is not a way to find it.
    if !enabled && !asked {
        return;
    }
    // Asked for, a project answers even when it has nothing to run, saying what it looked for.
    let Some(found) = command(Path::new(root)) else {
        if asked {
            emit(TurnChecks {
                status: CheckStatus::NotRun,
                command: None,
                duration_ms: 0,
                tests: None,
                failures: Vec::new(),
                output: None,
                note: Some(format!(
                    "No check command in this project. Looked for {WHAT}"
                )),
                enabled,
                at,
            });
        }
        return;
    };
    emit(TurnChecks {
        status: CheckStatus::Running,
        command: Some(found.line.clone()),
        duration_ms: 0,
        tests: None,
        failures: Vec::new(),
        output: None,
        note: None,
        enabled,
        at,
    });
    emit(finish(found, root, at, enabled));
}

/// Runs `found` in `root`, with [`LIMIT`], and says what it made of it.
pub(super) fn finish(found: Command, root: &str, at: u64, enabled: bool) -> TurnChecks {
    finish_within(found, root, at, LIMIT, enabled)
}

/// [`finish`] with its time limit given, so a test can watch one run out.
pub(super) fn finish_within(
    found: Command,
    root: &str,
    at: u64,
    limit: Duration,
    enabled: bool,
) -> TurnChecks {
    match run(root, &found.line, limit) {
        Ok(outcome) => {
            let tests = tests_ran(&outcome.output);
            // A command that exits zero having run nothing is not evidence: `cargo test` in a
            // crate without tests, or `node --test` in a folder without test files, both say so
            // and both exit zero. Saying "passed" for that is the guess this exists to avoid.
            let empty = outcome.passed && tests == Some(0);
            let status = if outcome.timed_out {
                CheckStatus::TimedOut
            } else if empty {
                CheckStatus::NotRun
            } else if outcome.passed {
                CheckStatus::Passed
            } else {
                CheckStatus::Failed
            };
            let mut failures = if status == CheckStatus::Failed {
                failures(&outcome.output)
            } else {
                Vec::new()
            };
            failures.truncate(MOST);
            TurnChecks {
                status,
                command: Some(found.line.clone()),
                duration_ms: outcome.duration.as_millis() as u64,
                tests,
                failures,
                output: tail(&outcome.output),
                note: if outcome.timed_out {
                    Some(format!(
                        "It was still running after {}, so it was stopped, and whether it would \
                         have passed is unknown.",
                        describe(limit)
                    ))
                } else if empty {
                    Some("It exited zero without running any tests, so nothing was checked.".into())
                } else {
                    None
                },
                enabled,
                at,
            }
        }
        Err(e) => TurnChecks {
            status: CheckStatus::NotRun,
            command: Some(found.line),
            tests: None,
            duration_ms: 0,
            failures: Vec::new(),
            output: None,
            note: Some(e.to_string()),
            enabled,
            at,
        },
    }
}

/// The project a thread belongs to, or None for a chat without one.
fn project_of(server: &Server, thread_id: &str) -> Option<String> {
    server.store.thread(thread_id)?.project_id
}

/// Whether a reply's turn is one to check: it finished, and it changed files.
pub(crate) fn worth_checking(
    store: &Store,
    thread_id: &str,
    message_id: &str,
    status: MessageStatus,
) -> bool {
    if status != MessageStatus::Complete {
        return false;
    }
    let Some(thread) = store.thread(thread_id) else {
        return false;
    };
    // A subagent's own transcript is not a reply the person reads on its own, and its work is
    // part of the turn that started it: that turn checks, once.
    if thread.project_id.is_none() || thread.parent.is_some() {
        return false;
    }
    thread
        .messages
        .iter()
        .find(|m| m.id == message_id)
        .is_some_and(|m| !m.files.is_empty())
}

/// A time limit in words: "5 minutes", "300 milliseconds".
fn describe(limit: Duration) -> String {
    let seconds = limit.as_secs();
    match seconds {
        0 => format!("{} milliseconds", limit.as_millis()),
        1 => "1 second".into(),
        2..=59 => format!("{seconds} seconds"),
        60 => "1 minute".into(),
        _ => format!("{} minutes", seconds / 60),
    }
}

/// The last [`KEEP`] bytes of `text` at a character boundary, with a line saying how much came
/// before when something was cut. None when it is empty.
pub(super) fn tail(text: &str) -> Option<String> {
    let text = text.trim_end();
    if text.is_empty() {
        return None;
    }
    if text.len() <= KEEP {
        return Some(text.to_string());
    }
    let mut start = text.len() - KEEP;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let cut = text[..start].lines().count();
    Some(format!(
        "…{cut} earlier lines, the end follows…\n{}",
        &text[start..]
    ))
}

/// What a project's check command is looked for, in the words a person reads when there is none.
const WHAT: &str = "package.json scripts (check, then test), Cargo.toml, pyproject.toml, go.mod, \
                    a Makefile's check or test target, and mix.exs.";

/// Whether the project runs its checks after every task, from Settings. Off unless it was turned
/// on, so a project's test suite is not run behind the person's back, on every task, the first
/// time they open it.
pub(crate) fn enabled(store: &Store, project_id: &str) -> bool {
    store.settings().checks.on(project_id)
}
