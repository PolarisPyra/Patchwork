//! The project's own check command: what to run, running it after a turn, and reading what it
//! said about the turn.
//!
//! ```text
//! detect  package.json (check, else test), Cargo.toml, pyproject.toml, go.mod, Makefile,
//!         mix.exs — the first one the project has, run with the package manager its lockfile
//!         names
//! run     through the same shell an agent's commands run in, in the project's folder, with a
//!         time limit; output is kept to its last bytes
//! parse   the failures the output names, each on its own; when it failed and named none, one
//!         claim for the command itself
//! ```
//!
//! Checks are the half of "nothing lands until you say so" that a person cannot do by reading a
//! diff: a reply says whether the project's tests still pass, and says "not run" rather than
//! nothing when they didn't run. A check that did not finish never counts as a pass.

mod detect;
mod parse;
mod run;

pub use detect::{command, Command};
pub use parse::{failures, tests_ran};
pub use run::{Outcome, run};

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::orchestration::{publish, AgentEvent};
use crate::persistence::Store;
use crate::system::now_ms;
use crate::threads::{CheckStatus, MessageStatus, TurnChecks};
use crate::Server;

/// How long the check command may run before it is stopped. Long enough for a cold build, short
/// enough that a hung command does not hold the reply's card at "Running" forever.
pub const LIMIT: Duration = Duration::from_secs(5 * 60);
/// Most of the output kept with a reply. The end is what names the failures.
const KEEP: usize = 16 << 10;
/// Most failures listed for one run.
const MOST: usize = 12;

impl Server {
    /// Runs the project's check command for the reply `message_id` of `thread_id`, on a thread of
    /// its own, publishing each change as it happens. The person's explicit request, so it runs
    /// even for a project whose checks are otherwise off.
    pub fn run_checks(self: &Arc<Self>, thread_id: &str, message_id: &str) -> Result<(), String> {
        let claim = claim(message_id).ok_or("The checks are still running for this reply.")?;
        let (server, thread, message) =
            (self.clone(), thread_id.to_string(), message_id.to_string());
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
        let (server, thread, message) =
            (self.clone(), thread_id.to_string(), message_id.to_string());
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
        )
    };
    check_project(&server.store, &project_id, &root, asked, now_ms(), &emit);
}

/// Checks the project at `root` and hands each card it comes to, in order, to `emit`: the running
/// one, then the result. Says nothing at all when the project has nothing to run, so a reply in a
/// folder without tests shows no checks rather than one that says it cannot be checked.
fn check_project(
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
                    "No check command in this project. Looked for {}",
                    WHAT
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
fn finish(found: Command, root: &str, at: u64, enabled: bool) -> TurnChecks {
    finish_within(found, root, at, LIMIT, enabled)
}

/// [`finish`] with its time limit given, so a test can watch one run out.
fn finish_within(
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
            note: Some(e),
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
fn tail(text: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_output_is_kept_whole() {
        assert_eq!(tail("ok\n").as_deref(), Some("ok"));
        assert_eq!(tail("   "), None);
    }

    fn project() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(crate::system::new_id());
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A failing command run for real: what it printed names the failure, and the reply keeps it.
    #[test]
    fn a_failing_check_names_what_failed() {
        let root = project();
        let checks = finish(
            Command {
                line: "echo 'FAILED tests/test_a.py::test_b - assert 1 == 2'; exit 1".into(),
            },
            &root.to_string_lossy(),
            7,
            false,
        );
        assert_eq!(checks.status, CheckStatus::Failed);
        assert_eq!(checks.failures.len(), 1);
        assert_eq!(checks.failures[0].name, "tests/test_a.py::test_b");
        assert_eq!(checks.failures[0].detail, "assert 1 == 2");
        assert!(checks.output.is_some(), "the output is kept for the card");
        assert_eq!(checks.at, 7);
        std::fs::remove_dir_all(root).ok();
    }

    /// A command that passes says so, and names no failure.
    #[test]
    fn a_passing_check_passes() {
        let root = project();
        let checks = finish(
            Command {
                line: "echo 'test result: ok. 3 passed; 0 failed'".into(),
            },
            &root.to_string_lossy(),
            0,
            false,
        );
        assert_eq!(checks.status, CheckStatus::Passed);
        assert!(checks.failures.is_empty());
        assert!(checks.output.is_some());
        std::fs::remove_dir_all(root).ok();
    }

    /// A command past its limit is stopped, says the result is unknown, and is not a pass.
    #[test]
    fn a_check_past_its_limit_is_not_a_pass() {
        let root = project();
        let checks = finish_within(
            Command {
                line: "sleep 30".into(),
            },
            &root.to_string_lossy(),
            0,
            Duration::from_millis(300),
            false,
        );
        assert_eq!(checks.status, CheckStatus::TimedOut);
        assert!(
            checks
                .note
                .as_deref()
                .is_some_and(|n| n.contains("300 milliseconds")),
            "{:?}",
            checks.note
        );
    }

    /// A store of its own, in a folder of its own.
    fn store() -> (crate::persistence::Store, std::path::PathBuf) {
        let dir = project();
        let store = crate::persistence::Store::open(dir.clone()).expect("a fresh database opens");
        (store, dir)
    }

    /// The whole path a reply's checks take, against a real project folder: the running card, then
    /// the failure the project's own `npm test` reported.
    #[test]
    fn a_turns_checks_run_the_project_command_and_name_what_failed() {
        let (store, data) = store();
        let root = project();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"demo","type":"module","scripts":{"test":"node --test"}}"#,
        )
        .unwrap();
        // A module and a test that fails, so the runner's real output is what gets read.
        std::fs::write(
            root.join("total.mjs"),
            "export const total = (items) => items.reduce((s, i) => s + i.price, 0);\n",
        )
        .unwrap();
        std::fs::write(
            root.join("total.test.mjs"),
            r#"import test from "node:test";
import assert from "node:assert/strict";
import { total } from "./total.mjs";
test("adds up prices", () => { assert.equal(total([{ price: 2 }, { price: 3 }]), 5); });
test("counts a discount", () => { assert.equal(total([{ price: 10 }, { price: -2 }]), 9); });
"#,
        )
        .unwrap();

        let cards = std::sync::Mutex::new(Vec::new());
        check_project(
            &store,
            "p1",
            &root.to_string_lossy(),
            true,
            now_ms(),
            &|checks| cards.lock().unwrap().push(checks),
        );
        let cards = cards.into_inner().unwrap();
        assert_eq!(cards.len(), 2, "the running card, then the result");
        assert_eq!(cards[0].status, CheckStatus::Running);
        assert_eq!(cards[0].command.as_deref(), Some("npm test"));

        let result = &cards[1];
        assert_eq!(result.status, CheckStatus::Failed, "{result:#?}");
        assert_eq!(result.failures.len(), 1, "{:#?}", result.failures);
        assert_eq!(result.failures[0].name, "counts a discount");
        assert!(
            result.failures[0].detail.contains("AssertionError"),
            "{:?}",
            result.failures[0].detail
        );
        assert!(result.duration_ms > 0);
        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(data).ok();
    }

    /// A project whose checks are off runs nothing when a turn ends, and says nothing at all:
    /// a card in every reply saying it did not check would be noise, and the switch is in
    /// Settings. Asked for, it runs them anyway.
    #[test]
    fn checks_that_are_off_say_nothing_and_run_when_asked() {
        let (store, data) = store();
        let root = project();
        std::fs::write(
            root.join("package.json"),
            r#"{"scripts":{"test":"exit 1"}}"#,
        )
        .unwrap();
        set_enabled_for(&store, "p1", false);

        let cards = std::sync::Mutex::new(Vec::new());
        check_project(
            &store,
            "p1",
            &root.to_string_lossy(),
            false,
            now_ms(),
            &|checks| cards.lock().unwrap().push(checks),
        );
        assert!(cards.into_inner().unwrap().is_empty(), "no card at all");

        // Asked for, the same project runs it anyway.
        let asked = std::sync::Mutex::new(Vec::new());
        check_project(
            &store,
            "p1",
            &root.to_string_lossy(),
            true,
            now_ms(),
            &|checks| asked.lock().unwrap().push(checks),
        );
        let asked = asked.into_inner().unwrap();
        assert_eq!(asked[1].status, CheckStatus::Failed);
        assert!(!asked[1].enabled, "the project's own setting is still off");
        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(data).ok();
    }

    /// A folder with nothing to run shows no card after a turn, and says what it looked for when
    /// the person asks.
    #[test]
    fn a_folder_with_nothing_to_run_says_why_when_asked() {
        let (store, data) = store();
        let root = project();
        let after = std::sync::Mutex::new(Vec::new());
        check_project(
            &store,
            "p1",
            &root.to_string_lossy(),
            false,
            now_ms(),
            &|checks| after.lock().unwrap().push(checks),
        );
        assert!(after.into_inner().unwrap().is_empty());

        let asked = std::sync::Mutex::new(Vec::new());
        check_project(
            &store,
            "p1",
            &root.to_string_lossy(),
            true,
            now_ms(),
            &|checks| asked.lock().unwrap().push(checks),
        );
        let asked = asked.into_inner().unwrap();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].status, CheckStatus::NotRun);
        assert!(
            asked[0]
                .note
                .as_deref()
                .is_some_and(|n| n.contains("package.json")),
            "{:?}",
            asked[0].note
        );
        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(data).ok();
    }

    /// A project follows the default until it says otherwise, and its own answer survives a reopen.
    #[test]
    fn a_projects_checks_setting_is_its_own() {
        let (store, data) = store();
        assert!(!enabled(&store, "p1"), "off until it is turned on");
        // The default, for every project that has not answered.
        store
            .update_settings(|s| s.checks.after_every_task = true)
            .unwrap();
        assert!(enabled(&store, "p1"));
        assert!(enabled(&store, "p2"), "the default covers every project");
        // One project answering for itself.
        store
            .update_settings(|s| {
                s.checks.projects.insert("p2".into(), false);
            })
            .unwrap();
        assert!(enabled(&store, "p1"));
        assert!(!enabled(&store, "p2"), "one project's answer is its own");
        drop(store);

        let store = crate::persistence::Store::open(data.clone()).unwrap();
        assert!(enabled(&store, "p1"), "the default survived the reopen");
        assert!(!enabled(&store, "p2"), "so did the project's own answer");
        std::fs::remove_dir_all(data).ok();
    }

    fn set_enabled_for(store: &crate::persistence::Store, project_id: &str, on: bool) {
        store
            .update_settings(|settings| {
                settings.checks.projects.insert(project_id.to_string(), on);
            })
            .unwrap();
    }

    /// Which replies a turn end checks at all: one that finished, in a project, having changed
    /// a file. Everything else leaves the project as it was, so there is nothing to check.
    #[test]
    fn only_a_finished_reply_that_changed_files_is_checked() {
        let (store, data) = store();
        // A task in a project, whose one reply changed a file.
        let thread = crate::threads::Thread {
            id: "t1".into(),
            project_id: Some("p1".into()),
            title: "a task".into(),
            agent: crate::provider::AgentKind::Claude,
            model: None,
            access_mode: crate::threads::AccessMode::FullAccess,
            effort: None,
            compact_window: None,
            session_id: None,
            created_at: 1,
            messages: vec![crate::threads::Message {
                id: "m1".into(),
                role: crate::threads::Role::Assistant,
                text: "done".into(),
                created_at: 1,
                status: MessageStatus::Complete,
                tools: vec![],
                plan: vec![],
                files: vec!["src/a.ts".into()],
                error: None,
                approvals: vec![],
                attachments: vec![],
                steer: false,
                queued: false,
                background: vec![],
                cut_off: false,
                refusal: None,
                parts: vec![],
                finished_at: None,
                usage: None,
                turn: None,
                checks: None,
            }],
            goal: None,
            position: 0,
            folder_id: None,
            archived_at: None,
            parent: None,
            plan_mode: false,
            slash_commands: vec![],
            imported_from: None,
            context_pending: false,
            role: None,
            role_pending: false,
        };
        let id = thread.messages[0].id.clone();
        store.insert_threads(vec![thread.clone()]).unwrap();
        assert!(worth_checking(
            &store,
            &thread.id,
            &id,
            MessageStatus::Complete
        ));
        assert!(
            !worth_checking(&store, &thread.id, &id, MessageStatus::Cancelled),
            "a stopped turn is not checked"
        );
        assert!(
            !worth_checking(&store, &thread.id, &id, MessageStatus::Error),
            "a failed turn is not checked"
        );

        store
            .update_thread(&thread.id, |t| {
                t.project_id = None;
                t.messages[0].files.clear();
            })
            .unwrap();
        assert!(
            !worth_checking(&store, &thread.id, &id, MessageStatus::Complete),
            "a chat without a project has no checks"
        );

        store
            .update_thread(&thread.id, |t| {
                t.project_id = Some("p1".into());
                t.messages[0].files = vec!["src/a.ts".into()];
                t.parent = Some(crate::threads::ThreadParent {
                    thread_id: "parent".into(),
                    tool_id: "tool".into(),
                });
            })
            .unwrap();
        assert!(
            !worth_checking(&store, &thread.id, &id, MessageStatus::Complete),
            "a subagent's transcript is checked by the turn that started it"
        );
        std::fs::remove_dir_all(data).ok();
    }

    /// A command that exits zero having run nothing: `cargo test` in a crate without tests. It is
    /// reported as not run, with the output kept, rather than as a pass.
    #[test]
    fn a_command_that_ran_no_tests_is_not_a_pass() {
        let root = project();
        let checks = finish(
            Command {
                line: "echo 'running 0 tests'; echo 'test result: ok. 0 passed; 0 failed'".into(),
            },
            &root.to_string_lossy(),
            0,
            false,
        );
        assert_eq!(checks.status, CheckStatus::NotRun, "{checks:#?}");
        assert_eq!(checks.tests, Some(0));
        assert!(
            checks
                .note
                .as_deref()
                .is_some_and(|n| n.contains("nothing")),
            "{:?}",
            checks.note
        );
        assert!(checks.output.is_some(), "what it printed is kept");
        std::fs::remove_dir_all(root).ok();
    }

    /// A check that runs tests says how many.
    #[test]
    fn a_passing_check_counts_the_tests_it_ran() {
        let root = project();
        let checks = finish(
            Command {
                line: "echo 'ℹ tests 3'; echo 'ℹ pass 3'".into(),
            },
            &root.to_string_lossy(),
            0,
            false,
        );
        assert_eq!(checks.status, CheckStatus::Passed, "{checks:#?}");
        assert_eq!(checks.tests, Some(3));
        assert!(checks.note.is_none());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_long_output_keeps_its_end() {
        let text = format!("{}\nthe last line", "line\n".repeat(10_000));
        let kept = tail(&text).expect("a long output keeps its end");
        assert!(kept.len() < KEEP + 100);
        assert!(kept.ends_with("the last line"));
        assert!(kept.starts_with('…'), "it says how much came before");
    }
}
