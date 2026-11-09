//! Tests for running a project's checks after a turn and reporting what they found.

// Rust guideline compliant 2026-10-09

use super::ops::*;
use super::*;
use std::time::Duration;
use crate::system::now_ms;
use crate::threads::{CheckStatus, MessageStatus};

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
    let _ = std::fs::remove_dir_all(root);
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
    let _ = std::fs::remove_dir_all(root);
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
    let store = crate::persistence::Store::open(&dir).expect("a fresh database opens");
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
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(data);
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
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(data);
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
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(data);
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

    let store = crate::persistence::Store::open(&data).unwrap();
    assert!(enabled(&store, "p1"), "the default survived the reopen");
    assert!(!enabled(&store, "p2"), "so did the project's own answer");
    let _ = std::fs::remove_dir_all(data);
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
        background: None,
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
        favorite: false,
        pinned: false,
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
    let _ = std::fs::remove_dir_all(data);
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
    let _ = std::fs::remove_dir_all(root);
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
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_long_output_keeps_its_end() {
    let text = format!("{}\nthe last line", "line\n".repeat(10_000));
    let kept = tail(&text).expect("a long output keeps its end");
    assert!(kept.len() < KEEP + 100);
    assert!(kept.ends_with("the last line"));
    assert!(kept.starts_with('…'), "it says how much came before");
}
