//! How the tracker follows the tasks an agent reports.

// Rust guideline compliant 2026-10-09

use crate::events::AgentEvent;
use crate::threads::TaskStatus;

use super::tracker::*;
use crate::provider::Parsed;
use crate::provider::TurnSpec;
use crate::provider::claude;
use crate::provider::codex;
use crate::provider::codex::tests as codex_fixtures;
use crate::threads::AccessMode;
use crate::provider::claude::tests::AGENT_LAUNCHED;
use crate::provider::claude::tests::AGENT_NOTIFIED;
use crate::provider::claude::tests::AGENT_STARTED;
use crate::provider::claude::tests::AGENT_UPDATED;
use crate::provider::claude::tests::BG_SHELL_LEVEL;
use crate::provider::claude::tests::BG_SHELL_LEVEL_EMPTY;
use crate::provider::claude::tests::BG_SHELL_LEVEL_STARTED;
use crate::provider::claude::tests::BG_SHELL_NOTIFIED;
use crate::provider::claude::tests::BG_SHELL_STARTED;
use crate::provider::claude::tests::BG_SHELL_UPDATED;
use crate::provider::claude::tests::OWNED_SHELL_NOTIFIED;
use crate::provider::claude::tests::OWNED_SHELL_STARTED;
use crate::provider::claude::tests::PROMOTED_ENDED;
use crate::provider::claude::tests::PROMOTED_RESULT;
use crate::provider::claude::tests::PROMOTED_STARTED;
use crate::provider::claude::tests::PROMOTED_UPDATED;
use crate::provider::claude::tests::TURN_RESULT;

/// Replays captured Claude lines the way the reader handles them: each task change goes to
/// `Background`, and each finished tool call goes through the same hold. Returns, for each
/// line, what it published and whether stdin stays open afterwards.
fn replay(background: &mut Background, lines: &[&str]) -> Vec<(Vec<AgentEvent>, bool)> {
    lines
        .iter()
        .map(|line| {
            let value = serde_json::from_str(line).unwrap();
            let mut events = Vec::new();
            for item in claude::parse(&value, "/abs", "m1") {
                match item {
                    Parsed::Task(event) => events.extend(background.apply(event, "m1")),
                    Parsed::Event(AgentEvent::ToolFinished {
                        tool_id, is_error, ..
                    }) => {
                        background.call_returned(&tool_id, "m1", is_error);
                    }
                    _ => {}
                }
            }
            (events, background.keeps_stdin_open())
        })
        .collect()
}

#[test]
fn a_turn_ending_while_a_subagent_runs_keeps_stdin_open_until_it_ends() {
    // The subagent's launch returns at once and its own command ends. The turn's reply ends
    // while the subagent still runs, and stdin must stay open for its report.
    let mut background = Background::default();
    let open = replay(
        &mut background,
        &[
            AGENT_STARTED,
            AGENT_LAUNCHED,
            OWNED_SHELL_STARTED,
            TURN_RESULT,
            OWNED_SHELL_NOTIFIED,
            AGENT_UPDATED,
            AGENT_NOTIFIED,
        ],
    );
    let open: Vec<bool> = open.into_iter().map(|(_, stdin)| stdin).collect();
    assert_eq!(open, [true, true, true, true, true, false, false]);
}

#[test]
fn a_status_and_a_notification_both_end_a_task_in_either_order() {
    // The status comes first, as it does in a live stream; the output arrives with the notification.
    let mut background = Background::default();
    let events = replay(
        &mut background,
        &[BG_SHELL_STARTED, BG_SHELL_UPDATED, BG_SHELL_NOTIFIED],
    );
    let [(started, _), (ended, _), (notified, _)] = events.as_slice() else {
        panic!("expected three replies");
    };
    assert!(matches!(
        started.as_slice(),
        [AgentEvent::BackgroundTaskStarted { .. }]
    ));
    assert!(matches!(
        ended.as_slice(),
        [AgentEvent::BackgroundTaskEnded { task, .. }] if task.output.is_none()
    ));
    // The notification keeps the status it has and adds the output, republishing the task.
    assert!(matches!(
        notified.as_slice(),
        [AgentEvent::BackgroundTaskUpdated { task, .. }]
            if task.status == TaskStatus::Done && task.output.is_some()
    ));
}

#[test]
fn a_notification_before_the_status_keeps_its_output() {
    let mut background = Background::default();
    let events = replay(
        &mut background,
        &[BG_SHELL_STARTED, BG_SHELL_NOTIFIED, BG_SHELL_UPDATED],
    );
    let [(_, _), (notified, _), (ended, _)] = events.as_slice() else {
        panic!("expected three replies");
    };
    assert!(matches!(
        notified.as_slice(),
        [AgentEvent::BackgroundTaskEnded { task, .. }] if task.output.is_some()
    ));
    // The status that comes second does not lose the output the notification carried.
    assert!(matches!(
        ended.as_slice(),
        [AgentEvent::BackgroundTaskUpdated { task, .. }] if task.output.is_some()
    ));
}

#[test]
fn a_promoted_command_is_listed_when_the_agent_moves_it_to_the_background() {
    let mut background = Background::default();
    let events = replay(
        &mut background,
        &[
            PROMOTED_STARTED,
            PROMOTED_UPDATED,
            PROMOTED_RESULT,
            PROMOTED_ENDED,
        ],
    );
    // Started in the foreground, it is not listed until the agent moves it.
    assert!(events[0].0.is_empty());
    assert!(matches!(
        events[1].0.as_slice(),
        [AgentEvent::BackgroundTaskStarted { task, .. }]
            if task.task_id.as_deref() == Some("ba28p82m4") && task.kind == "local_bash"
    ));
    assert!(matches!(
        events[3].0.as_slice(),
        [AgentEvent::BackgroundTaskEnded { task, .. }] if task.status == TaskStatus::Done
    ));
}

#[test]
fn a_held_call_is_shown_finished_when_its_task_ends() {
    let mut background = Background::default();
    let events = replay(&mut background, &[AGENT_STARTED, AGENT_LAUNCHED]);
    assert!(
        events[0]
            .0
            .iter()
            .any(|e| matches!(e, AgentEvent::BackgroundTaskStarted { .. }))
    );
    // The launch's result was held: no finish was published for the call yet.
    assert!(events[1].0.is_empty());

    // The task's end finishes the call, in the same publish as the task's own end.
    let ended = replay(&mut background, &[AGENT_UPDATED, AGENT_NOTIFIED]);
    assert!(matches!(
        ended[0].0.as_slice(),
        [
            AgentEvent::BackgroundTaskEnded { .. },
            AgentEvent::ToolFinished { tool_id, is_error: false, .. },
        ] if tool_id == "toolu_01GQWb3a5rvJrVhxJtrPz2BN"
    ));
}

#[test]
fn a_task_the_level_drops_ends_even_when_its_own_end_never_arrived() {
    // The captured level, its start line, and the level that drops it. The task's own end
    // (its status patch and its notification) is left out, as it is when one is lost: the
    // level is the only report that still says the task is over.
    let mut background = Background::default();
    let lines = replay(
        &mut background,
        &[BG_SHELL_LEVEL, BG_SHELL_LEVEL_STARTED, BG_SHELL_LEVEL_EMPTY],
    );
    // The first level names the task before its start line, so it lists nothing.
    assert!(lines[0].0.is_empty());
    assert!(matches!(
        lines[1].0.as_slice(),
        [AgentEvent::BackgroundTaskStarted { task, .. }]
            if task.task_id.as_deref() == Some("bad1lsfze")
    ));
    assert!(matches!(
        lines[2].0.as_slice(),
        [AgentEvent::BackgroundTaskEnded { task, .. }] if task.status == TaskStatus::Stopped
    ));
    // The task no longer holds the turn's stdin open, so the turn can end.
    assert!(!lines[2].1);
    assert!(!background.any_listed_live());
}

#[test]
fn a_level_that_still_names_the_task_leaves_it_running() {
    let mut background = Background::default();
    let events = replay(&mut background, &[BG_SHELL_LEVEL_STARTED, BG_SHELL_LEVEL]);
    assert!(matches!(
        events[0].0.as_slice(),
        [AgentEvent::BackgroundTaskStarted { .. }]
    ));
    assert!(events[1].0.is_empty());
    assert!(background.keeps_stdin_open() && background.any_listed_live());
}

#[test]
fn a_process_that_ends_stops_its_open_tasks_and_their_calls() {
    let mut background = Background::default();
    replay(&mut background, &[AGENT_STARTED, AGENT_LAUNCHED]);
    let events = background.stop_all();
    assert!(matches!(
        events.as_slice(),
        [
            AgentEvent::BackgroundTaskEnded { task, .. },
            AgentEvent::ToolFinished { is_error: true, .. },
        ] if task.status == TaskStatus::Stopped
    ));
    assert!(!background.keeps_stdin_open());
}

/// Replays captured Codex lines the way the reader handles them: a call that starts a command
/// is a task that reports no end, and its turn completing lists the tasks still open.
#[test]
fn a_codex_command_open_at_turn_end_is_listed_and_holds_no_stdin() {
    let spec = TurnSpec {
        root: "/abs/work",
        host: None,
        thread_id: "thread",
        session_id: None,
        model: None,
        vision_model: None,
        text: "go",
        mode: AccessMode::FullAccess,
        plan: false,
        effort: None,
        compact_window: 0,
        unlimited_steps: false,
        token_limit: 0,
        attachments: &[],
        images: &[],
        instructions: None,
        skills_dir: None,
        guarded: &[],
        agents: &[],
        hooks: &[],
    };
    let mut background = Background::default();
    let mut events = Vec::new();
    for line in [
        codex_fixtures::QUICK_STARTED,
        codex_fixtures::QUICK_COMPLETED,
        codex_fixtures::COMMAND_STARTED,
        codex_fixtures::TURN_COMPLETED,
    ] {
        let value = serde_json::from_str(line).unwrap();
        for item in codex::parse(&value, &spec, "m1") {
            match item {
                Parsed::Event(AgentEvent::ToolStarted { tool, .. }) => {
                    if let Some(task) = codex::background_task(&tool) {
                        background.start_unreported(task);
                    }
                }
                Parsed::Event(AgentEvent::ToolFinished {
                    tool_id, is_error, ..
                }) => events.extend(background.finish_unreported(&tool_id, is_error)),
                Parsed::Finished => events.extend(background.list_unreported("m1")),
                _ => {}
            }
        }
    }
    // Only the command still running at turn end is listed; the quick one finished.
    assert!(matches!(
        events.as_slice(),
        [AgentEvent::BackgroundTaskStarted { task, .. }]
            if task.tool_id == "exec-d7e8f3a5-1046-4c24-9b3b-f176ac51ae9c" && task.kind == "command"
    ));
    // Codex never reports it ending, so nothing waits on it: stdin closes with the turn.
    assert!(!background.keeps_stdin_open());
}
