//! Tests for the database: adopting old data, and what survives a reopen.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use super::*;
use crate::provider::AgentKind;
use crate::system::new_id;
use crate::threads::{Message, MessageStatus, Role, Thread};

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(new_id());
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn message(text: &str, status: MessageStatus) -> Message {
    Message {
        id: new_id(),
        role: Role::Assistant,
        text: text.into(),
        created_at: 1,
        status,
        tools: vec![],
        plan: vec![],
        files: vec!["a.rs".into()],
        error: None,
        approvals: vec![],
        attachments: vec![],
        steer: false,
        queued: false,
        background: Vec::new(),
        cut_off: false,
        refusal: None,
        parts: Vec::new(),
        finished_at: None,
        usage: None,
        turn: None,
        checks: None,
        cursor: None,
    }
}

fn thread(messages: Vec<Message>) -> Thread {
    Thread {
        id: new_id(),
        project_id: None,
        title: "t".into(),
        agent: AgentKind::Antigravity,
        model: Some("pro".into()),
        session_id: Some("s".into()),
        access_mode: crate::threads::AccessMode::Supervised,
        created_at: 2,
        messages: messages.into_iter().map(std::sync::Arc::new).collect(),
        goal: None,
        position: 0,
        folder_id: None,
        effort: None,
        compact_window: None,
        background: None,
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
    }
}

#[test]
fn legacy_json_moves_into_sqlite() {
    let dir = temp_dir();
    std::fs::write(
        dir.join("settings.json"),
        r#"{"onboarded":true,"agent":"codex","projects":[{"id":"p","name":"n","path":"/x"}]}"#,
    )
    .unwrap();
    let t = thread(vec![message("hi", MessageStatus::Streaming)]);
    std::fs::write(
        dir.join("threads.json"),
        serde_json::to_string(&[&t]).unwrap(),
    )
    .unwrap();

    let store = Store::open(&dir).unwrap();
    let s = store.settings();
    assert_eq!(s.agents, [AgentKind::Codex]);
    assert_eq!(s.projects[0].path, "/x");
    let loaded = store.thread(&t.id).unwrap();
    assert_eq!(loaded.messages[0].status, MessageStatus::Interrupted);
    assert_eq!(loaded.messages[0].files, ["a.rs"]);
    assert!(dir.join("settings.json.bak").exists() && !dir.join("threads.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn thread_changes_survive_reopen_and_delete_cascades() {
    let dir = temp_dir();
    let store = Store::open(&dir).unwrap();
    let t = thread(vec![message("a", MessageStatus::Complete)]);
    store.insert_threads(vec![t.clone()]).unwrap();
    store
        .update_thread(&t.id, |t| {
            t.title = "renamed".into();
            t.slash_commands = vec![crate::provider::ProviderSlashCommand {
                name: "review".into(),
                description: Some("Review the current changes".into()),
            }];
            std::sync::Arc::make_mut(&mut t.messages[0]).text.push('b');
            t.messages
                .push(std::sync::Arc::new(message("c", MessageStatus::Complete)));
        })
        .unwrap();
    drop(store);

    let store = Store::open(&dir).unwrap();
    let loaded = store.thread(&t.id).unwrap();
    assert_eq!(loaded.title, "renamed");
    assert_eq!(loaded.slash_commands[0].name, "review");
    assert_eq!(
        loaded.slash_commands[0].description.as_deref(),
        Some("Review the current changes")
    );
    let texts: Vec<_> = loaded.messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, ["ab", "c"]);
    assert_eq!(
        store.session_keys(),
        [(AgentKind::Antigravity, "s".to_string())]
    );

    store.delete_thread(&t.id).unwrap();
    let count: i64 = store
        .lock()
        .conn
        .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let _ = std::fs::remove_dir_all(&dir);
}
/// Computer use is a setting, so it survives a restart. The flag the tools read is mirrored
/// from it; that mirror is checked on its own, since every `Store` in a test process writes
/// it and the tests run side by side.
#[test]
fn computer_use_is_a_setting_and_a_mirror() {
    let dir = temp_dir();
    let store = Store::open(&dir).unwrap();
    assert!(!store.settings().computer_use, "off until it is turned on");
    store.update_settings(|s| s.computer_use = true).unwrap();
    drop(store);

    let store = Store::open(&dir).unwrap();
    assert!(store.settings().computer_use);
    store.update_settings(|s| s.computer_use = false).unwrap();
    assert!(!store.settings().computer_use);

    crate::computer::set_enabled(true);
    assert!(crate::computer::enabled());
    crate::computer::set_enabled(false);
    assert!(!crate::computer::enabled());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scheduled_tasks_survive_reopen() {
    use crate::scheduled::{Schedule, Task, Weekday};

    let dir = temp_dir();
    let store = Store::open(&dir).unwrap();
    let task = Task {
        id: String::new(),
        name: "Weekly report".into(),
        prompt: "Report on what landed.".into(),
        schedule: Schedule::Weekly {
            days: vec![Weekday::Mon, Weekday::Fri],
            time: "09:00".into(),
        },
        project_id: Some("p".into()),
        agent: AgentKind::Codex,
        model: None,
        access_mode: crate::threads::AccessMode::Auto,
        enabled: true,
        last_run_at: None,
        next_run_at: None,
        created_at: 7,
        recent_runs: Vec::new(),
    };
    let saved = crate::scheduled::prepare(task).unwrap();
    store.save_scheduled_task(&saved).unwrap();
    assert!(!saved.id.is_empty(), "a new task is given an id");
    assert!(saved.next_run_at.is_some(), "a saved task is scheduled");
    drop(store);

    let store = Store::open(&dir).unwrap();
    let tasks = store.scheduled_tasks();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "Weekly report");
    assert_eq!(tasks[0].agent, AgentKind::Codex);
    assert_eq!(tasks[0].schedule, saved.schedule);
    assert!(store.has_scheduled_task(&saved.id));

    store
        .scheduled_task_ran(&saved.id, 99, Some(123), Ok("thread"))
        .unwrap();
    assert_eq!(store.scheduled_tasks()[0].last_run_at, Some(99));
    assert_eq!(store.scheduled_tasks()[0].next_run_at, Some(123));

    // A skipped slot is dated by the slot it missed; history keeps the newest five.
    store.scheduled_task_skipped(&saved.id, Some(456)).unwrap();
    assert_eq!(store.scheduled_tasks()[0].next_run_at, Some(456));
    for at in 200..205 {
        store
            .scheduled_task_ran(&saved.id, at, Some(456), Err("no agent"))
            .unwrap();
    }
    let runs = &store.scheduled_tasks()[0].recent_runs;
    let ats: Vec<i64> = runs.iter().map(|run| run.at).collect();
    assert_eq!(ats, [204, 203, 202, 201, 200]);
    assert_eq!(runs[0].outcome, crate::scheduled::RunOutcome::Failed);
    assert_eq!(runs[0].detail.as_deref(), Some("no agent"));
    store
        .scheduled_task_ran(&saved.id, 100, Some(456), Ok("thread"))
        .unwrap();
    store.scheduled_task_skipped(&saved.id, Some(789)).unwrap();
    let runs = &store.scheduled_tasks()[0].recent_runs;
    assert_eq!(runs[0].at, 456, "the skipped slot");
    assert_eq!(runs[0].outcome, crate::scheduled::RunOutcome::Skipped);

    store.remove_scheduled_task(&saved.id).unwrap();
    assert!(store.scheduled_tasks().is_empty());
    assert!(!store.has_scheduled_task(&saved.id));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_task_without_a_name_or_a_time_is_refused() {
    use crate::scheduled::{Schedule, Task};

    let dir = temp_dir();
    let store = Store::open(&dir).unwrap();
    let task = |name: &str, time: &str| Task {
        id: String::new(),
        name: name.into(),
        prompt: "p".into(),
        schedule: Schedule::Daily { time: time.into() },
        project_id: None,
        agent: AgentKind::Claude,
        model: None,
        access_mode: crate::threads::AccessMode::Supervised,
        enabled: true,
        last_run_at: None,
        next_run_at: None,
        created_at: 0,
        recent_runs: Vec::new(),
    };
    crate::scheduled::prepare(task("", "09:00")).unwrap_err();
    crate::scheduled::prepare(task("n", "nine")).unwrap_err();
    assert!(store.scheduled_tasks().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A reply's checks are part of the reply: they survive a restart, and a reply whose checks did
/// not run keeps saying so.
#[test]
fn a_replys_checks_survive_reopen() {
    let dir = temp_dir();
    let store = Store::open(&dir).unwrap();
    let mut t = thread(vec![message("done", MessageStatus::Complete)]);
    std::sync::Arc::make_mut(&mut t.messages[0]).checks = Some(crate::threads::TurnChecks {
        status: crate::threads::CheckStatus::Failed,
        command: Some("cargo test".into()),
        duration_ms: 1234,
        tests: Some(3),
        failures: vec![crate::threads::CheckFailure {
            name: "math::subtracts".into(),
            detail: "assertion `left == right` failed".into(),
        }],
        output: Some("test result: FAILED. 2 passed; 1 failed".into()),
        note: None,
        enabled: true,
        at: 42,
    });
    store.insert_threads(vec![t.clone()]).unwrap();
    drop(store);

    let store = Store::open(&dir).unwrap();
    let loaded = store.thread(&t.id).unwrap();
    let checks = loaded.messages[0].checks.clone().expect("kept");
    assert_eq!(checks.status, crate::threads::CheckStatus::Failed);
    assert_eq!(checks.command.as_deref(), Some("cargo test"));
    assert_eq!(checks.failures[0].name, "math::subtracts");
    assert!(checks.enabled);
    assert_eq!(checks.tests, Some(3));
    assert_eq!(checks.at, 42);
    let _ = std::fs::remove_dir_all(&dir);
}
