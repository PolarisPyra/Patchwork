use super::*;

use super::parse::{claude_ended_status, claude_task_status};
use std::collections::HashMap;

use serde_json::Value;

use crate::threads::AccessMode;
use crate::orchestration::AgentEvent;

use crate::threads::StepStatus;

use crate::threads::TaskStatus;

use crate::provider::Parsed;
use crate::provider::TaskEvent;

use crate::provider::TurnSpec;
use serde_json::json;

#[test]
fn claude_init_is_session_and_noise_is_ignored() {
    let init = json!({"type": "system", "subtype": "init", "cwd": "/abs", "session_id": "9578"});
    let out = parse(&init, "/abs", "m1");
    assert!(
        out.iter()
            .any(|item| matches!(item, Parsed::SessionId(id) if id == "9578"))
    );
    assert!(
        out.iter()
            .any(|item| matches!(item, Parsed::SlashCommands(commands) if commands.is_empty()))
    );

    let hook = json!({"type": "system", "subtype": "hook_started"});
    assert!(parse(&hook, "/abs", "m1").is_empty());
    let rate = json!({"type": "rate_limit_event"});
    assert!(parse(&rate, "/abs", "m1").is_empty());
}

#[test]
fn init_advertises_native_slash_commands() {
    let init = json!({"type":"system","subtype":"init","session_id":"s","slash_commands":["compact","review"]});
    let parsed = parse(&init, "/", "m");
    assert!(parsed.iter().any(|item| matches!(item, Parsed::SlashCommands(commands)
        if commands.iter().map(|command| command.name.as_str()).collect::<Vec<_>>() == ["compact", "review"])));
}

#[test]
fn claude_assistant_text_and_edit() {
    let v = json!({"type": "assistant", "message": {"content": [
        {"type": "text", "text": "hi"},
        {"type": "thinking", "thinking": "..."},
        {"type": "tool_use", "id": "toolu_1", "name": "Edit",
         "input": {"file_path": "/abs/a.css", "old_string": "..", "new_string": ".."}}
    ]}});
    let out = parse(&v, "/abs", "m1");
    // The text block was already streamed as deltas, so only the tool calls remain.
    assert_eq!(out.len(), 2);
    assert!(matches!(
        &out[0],
        Parsed::Event(AgentEvent::ToolStarted { tool, .. })
            if tool.id == "toolu_1" && tool.name == "Edit" && tool.summary == "a.css"
                && tool.status == StepStatus::Running
    ));
    assert!(
        matches!(&out[1], Parsed::Event(AgentEvent::FileTouched { path, .. }) if path == "a.css")
    );
}

#[test]
fn claude_todo_write_becomes_plan() {
    let v = json!({"type": "assistant", "message": {"content": [
        {"type": "tool_use", "id": "t2", "name": "TodoWrite", "input": {"todos": [
            {"content": "Plan", "status": "completed"},
            {"content": "Build", "status": "in_progress"},
            {"content": "Ship", "status": "pending"}
        ]}}
    ]}});
    let out = parse(&v, "/abs", "m1");
    let [_, Parsed::Event(AgentEvent::PlanUpdated { steps, .. })] = out.as_slice() else {
        panic!("expected a plan update, got {out:?}");
    };
    let statuses: Vec<StepStatus> = steps.iter().map(|s| s.status).collect();
    assert_eq!(
        statuses,
        [StepStatus::Done, StepStatus::Running, StepStatus::Pending]
    );
}

#[test]
fn claude_tool_result_and_result_errors() {
    let user = json!({"type": "user", "message": {"content": [
        {"type": "tool_result", "tool_use_id": "toolu_1", "is_error": true, "content": "x"}
    ]}});
    let out = parse(&user, "/abs", "m1");
    assert!(matches!(
        out.as_slice(),
        [Parsed::Event(AgentEvent::ToolFinished { tool_id, is_error: true, .. })] if tool_id == "toolu_1"
    ));

    let failed =
        json!({"type": "result", "subtype": "success", "is_error": true, "result": "Invalid key"});
    let out = parse(&failed, "/abs", "m1");
    assert!(matches!(out.as_slice(), [Parsed::Error(e), Parsed::Finished] if e == "Invalid key"));

    let ok = json!({"type": "result", "subtype": "success", "is_error": false, "result": "hi"});
    assert!(matches!(
        parse(&ok, "/abs", "m1").as_slice(),
        [Parsed::Finished]
    ));
}

#[test]
fn claude_permission_request_round_trip() {
    let request = json!({"type": "control_request", "request_id": "r1", "request": {
        "subtype": "can_use_tool", "tool_name": "Bash", "display_name": "Bash",
        "input": {"command": "rm -rf build"}}});
    let out = parse(&request, "/abs", "m1");
    let [
        Parsed::Approval {
            title,
            detail,
            reply,
            ..
        },
    ] = out.as_slice()
    else {
        panic!("{out:?}");
    };
    assert_eq!(
        (title.as_str(), detail.as_str()),
        ("Run command", "rm -rf build")
    );

    let allow: Value = serde_json::from_str(&approval_reply(reply, true, &HashMap::new())).unwrap();
    assert_eq!(allow["response"]["request_id"], "r1");
    assert_eq!(allow["response"]["response"]["behavior"], "allow");
    assert_eq!(
        allow["response"]["response"]["updatedInput"]["command"],
        "rm -rf build"
    );
    let deny: Value = serde_json::from_str(&approval_reply(reply, false, &HashMap::new())).unwrap();
    assert_eq!(deny["response"]["response"]["behavior"], "deny");

    let other = json!({"type": "control_request", "request_id": "r2",
        "request": {"subtype": "hook_callback"}});
    let out = parse(&other, "/abs", "m1");
    assert!(matches!(out.as_slice(), [Parsed::Send(line)] if line.contains("\"error\"")));
}

#[test]
fn claude_args_include_resume_model_and_mode() {
    let mut spec = TurnSpec {
        root: "/abs",
        thread_id: "thread",
        session_id: Some("sid"),
        model: Some("opus"),
        vision_model: None,
        text: "do it",
        mode: AccessMode::FullAccess,
        plan: false,
        effort: None,
        compact_window: 0,
        attachments: &[],
        images: &[],
        instructions: None,
        skills_dir: None,
        guarded: &[],
        agents: &[],
        hooks: &[],
    };
    assert_eq!(
        turn_args(&spec),
        [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--forward-subagent-text",
            "--permission-mode",
            "bypassPermissions",
            "--permission-prompt-tool",
            "stdio",
            "--resume",
            "sid",
            "--model",
            "opus",
        ]
    );
    spec.mode = AccessMode::Supervised;
    let args = turn_args(&spec);
    assert!(
        args.windows(2)
            .any(|w| w == ["--permission-mode", "manual"])
    );
    assert!(
        args.windows(2)
            .any(|w| w == ["--permission-prompt-tool", "stdio"])
    );
}

fn parsed(line: &str) -> Vec<Parsed> {
    parse(&serde_json::from_str(line).unwrap(), "/abs", "m1")
}

#[test]
fn a_background_command_starts_listed_with_its_identity() {
    let out = parsed(BG_SHELL_STARTED);
    let [
        Parsed::Task(TaskEvent::Started {
            task,
            task_id,
            top_level,
            backgrounded,
        }),
    ] = out.as_slice()
    else {
        panic!("expected one started task, got {out:?}");
    };
    assert_eq!(task_id.as_str(), "bsktq4olb");
    assert!(*top_level && *backgrounded);
    assert_eq!(task.tool_id, "toolu_01UcFx54BgAVg64YDxNNvZHh");
    assert_eq!(task.task_id.as_deref(), Some("bsktq4olb"));
    assert_eq!(task.run_id.as_deref(), Some("0mvsny618-461e82c2"));
    assert_eq!(
        (task.kind.as_str(), task.status),
        ("local_bash", TaskStatus::Running)
    );
    assert!(task.started_at > 0);
}

#[test]
fn a_status_patch_ends_the_task_and_its_notification_adds_the_output() {
    let out = parsed(BG_SHELL_UPDATED);
    assert!(matches!(
        out.as_slice(),
        [Parsed::Task(TaskEvent::Ended { task_id, status: TaskStatus::Done, summary: None, output: None })]
            if task_id.as_str() == "bsktq4olb"
    ));

    let out = parsed(BG_SHELL_NOTIFIED);
    let [
        Parsed::Task(TaskEvent::Ended {
            status: TaskStatus::Done,
            summary: Some(summary),
            output: Some(output),
            ..
        }),
    ] = out.as_slice()
    else {
        panic!("expected the notification to carry the output, got {out:?}");
    };
    assert!(summary.starts_with("Background command"));
    assert!(output.ends_with("/tasks/bsktq4olb.output"));
}

#[test]
fn a_subagents_own_command_is_not_top_level_and_has_no_output_file() {
    let out = parsed(OWNED_SHELL_STARTED);
    assert!(matches!(
        out.as_slice(),
        [Parsed::Task(TaskEvent::Started {
            top_level: false,
            backgrounded: false,
            ..
        })]
    ));
    // The CLI sends an empty output file name for it, which is no output.
    let out = parsed(OWNED_SHELL_NOTIFIED);
    assert!(matches!(
        out.as_slice(),
        [Parsed::Task(TaskEvent::Ended {
            status: TaskStatus::Done,
            output: None,
            ..
        })]
    ));
}

#[test]
fn a_subagents_progress_updates_it_and_never_ends_it() {
    // This capture names the progress text `description`, with no `summary`.
    let out = parsed(AGENT_PROGRESS);
    assert!(matches!(
        out.as_slice(),
        [Parsed::Task(TaskEvent::Progress { task_id, summary })]
            if task_id.as_str() == "adf0968466f0fada1" && summary.as_str() == "Running Sleep for 4 seconds"
    ));
}

#[test]
fn a_foreground_command_moved_to_the_background_is_listed_by_the_patch() {
    let out = parsed(PROMOTED_STARTED);
    assert!(matches!(
        out.as_slice(),
        [Parsed::Task(TaskEvent::Started {
            top_level: true,
            backgrounded: false,
            ..
        })]
    ));
    let out = parsed(PROMOTED_UPDATED);
    assert!(matches!(
        out.as_slice(),
        [Parsed::Task(TaskEvent::Updated { task_id, status: None, description: None, backgrounded: true })]
            if task_id.as_str() == "ba28p82m4"
    ));
    let out = parsed(PROMOTED_ENDED);
    assert!(matches!(
        out.as_slice(),
        [Parsed::Task(TaskEvent::Ended {
            status: TaskStatus::Done,
            output: None,
            ..
        })]
    ));
    // Its notification comes after the status, and names the file the output went to.
    let out = parsed(PROMOTED_NOTIFIED);
    assert!(matches!(
        out.as_slice(),
        [Parsed::Task(TaskEvent::Ended {
            status: TaskStatus::Done,
            summary: Some(_),
            output: Some(_),
            ..
        })]
    ));
}

#[test]
fn the_level_of_live_tasks_is_read_as_the_ids_it_names() {
    let out = parsed(BG_SHELL_LEVEL);
    assert!(matches!(
        out.as_slice(),
        [Parsed::Task(TaskEvent::Level { live })]
            if live.as_slice() == ["bad1lsfze".to_string()]
    ));
    let out = parsed(BG_SHELL_LEVEL_EMPTY);
    assert!(matches!(
        out.as_slice(),
        [Parsed::Task(TaskEvent::Level { live })] if live.is_empty()
    ));
}

#[test]
fn task_statuses_map_to_live_or_ended() {
    assert_eq!(claude_task_status("running"), TaskStatus::Running);
    assert_eq!(claude_task_status("pending"), TaskStatus::Waiting);
    assert_eq!(claude_task_status("paused"), TaskStatus::Waiting);
    assert_eq!(claude_ended_status("completed"), TaskStatus::Done);
    assert_eq!(claude_ended_status("failed"), TaskStatus::Failed);
    assert_eq!(claude_ended_status("killed"), TaskStatus::Stopped);
}

// Lines the CLI sent, captured from Claude Code 2.1.310 on a Haiku turn: a background command, a
// background subagent with its own command, and a foreground command moved to the background.
// Task lines are verbatim; the tool results and the turn result are trimmed to the fields the
// parser reads.
pub const BG_SHELL_STARTED: &str = r#"{"type":"system","subtype":"task_started","task_id":"bsktq4olb","run_id":"0mvsny618-461e82c2","tool_use_id":"toolu_01UcFx54BgAVg64YDxNNvZHh","description":"Run a short sleep in the background and echo a marker","is_backgrounded":true,"task_type":"local_bash","uuid":"5a0ad893-e218-4031-b435-641d1c677cda","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
pub const BG_SHELL_UPDATED: &str = r#"{"type":"system","subtype":"task_updated","task_id":"bsktq4olb","run_id":"0mvsny618-461e82c2","patch":{"status":"completed","end_time":1793225143763},"uuid":"55c176bd-ee9b-4e94-9d7c-852caf9d7e30","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
pub const BG_SHELL_NOTIFIED: &str = r#"{"type":"system","subtype":"task_notification","task_id":"bsktq4olb","run_id":"0mvsny618-461e82c2","tool_use_id":"toolu_01UcFx54BgAVg64YDxNNvZHh","status":"completed","output_file":"/tmp/claude-1000/-home-user-project/4d035a83-ccf0-4211-bc1a-8e16e2e5cd55/tasks/bsktq4olb.output","summary":"Background command \"Run a short sleep in the background and echo a marker\" completed (exit code 0)","uuid":"b2d4b658-92ea-4b5c-9e96-deaa6d7c9425","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
pub const AGENT_STARTED: &str = r#"{"type":"system","subtype":"task_started","task_id":"adf0968466f0fada1","run_id":"0mvsny6bc-f97bc88a","tool_use_id":"toolu_01GQWb3a5rvJrVhxJtrPz2BN","description":"Haiku sleep check","subagent_type":"general-purpose","is_backgrounded":true,"spawn_depth":1,"task_type":"local_agent","prompt":"Run sleep 4 with Bash and reply OK.","uuid":"e0045c4e-625e-4882-9bd3-1f980726596b","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
pub const AGENT_PROGRESS: &str = r#"{"type":"system","subtype":"task_progress","task_id":"adf0968466f0fada1","run_id":"0mvsny6bc-f97bc88a","tool_use_id":"toolu_01GQWb3a5rvJrVhxJtrPz2BN","description":"Running Sleep for 4 seconds","subagent_type":"general-purpose","usage":{"total_tokens":20624,"tool_uses":1,"duration_ms":2155},"last_tool_name":"Bash","uuid":"913f4847-1809-46dd-b614-c3ca458f0e4e","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
pub const OWNED_SHELL_STARTED: &str = r#"{"type":"system","subtype":"task_started","task_id":"btmpqjc76","run_id":"0mvsnyaam-40cbdd66","owned_by_subagent":true,"tool_use_id":"toolu_017nYGv4WtxfVr2PMXvRukas","description":"Sleep for 4 seconds","is_backgrounded":false,"parent_task_id":"adf0968466f0fada1","task_type":"local_bash","uuid":"9e33aea8-1a5a-41b3-a8f5-1dd6b02e2bb5","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
pub const OWNED_SHELL_NOTIFIED: &str = r#"{"type":"system","subtype":"task_notification","task_id":"btmpqjc76","run_id":"0mvsnyaam-40cbdd66","tool_use_id":"toolu_017nYGv4WtxfVr2PMXvRukas","status":"completed","output_file":"","summary":"Sleep for 4 seconds","uuid":"a7d7c6a2-f384-43a8-b1d2-e3a901ec2a37","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
pub const AGENT_UPDATED: &str = r#"{"type":"system","subtype":"task_updated","task_id":"adf0968466f0fada1","run_id":"0mvsny6bc-f97bc88a","patch":{"status":"completed","end_time":1793225148310},"uuid":"2365cd5c-0c13-4d93-b120-317608496eee","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
pub const AGENT_NOTIFIED: &str = r#"{"type":"system","subtype":"task_notification","task_id":"adf0968466f0fada1","run_id":"0mvsny6bc-f97bc88a","tool_use_id":"toolu_01GQWb3a5rvJrVhxJtrPz2BN","status":"completed","output_file":"/tmp/claude-1000/-home-user-project/4d035a83-ccf0-4211-bc1a-8e16e2e5cd55/tasks/adf0968466f0fada1.output","summary":"OK","usage":{"total_tokens":21288,"tool_uses":1,"duration_ms":7201},"uuid":"adbb5608-381d-4695-9154-a9cb5bb28d6e","session_id":"4d035a83-ccf0-4211-bc1a-8e16e2e5cd55"}"#;
pub const AGENT_LAUNCHED: &str = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01GQWb3a5rvJrVhxJtrPz2BN","content":"Async agent launched successfully."}]},"parent_tool_use_id":null}"#;
pub const TURN_RESULT: &str =
    r#"{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}"#;
pub const PROMOTED_STARTED: &str = r#"{"type":"system","subtype":"task_started","task_id":"ba28p82m4","run_id":"0mvso3an8-e7937c61","tool_use_id":"toolu_015w8zbEatreDdpozAygHZzs","description":"node -e \"setTimeout(() => console.log(\\\"fg-done\\\"), 125000)\"","is_backgrounded":false,"task_type":"local_bash","uuid":"ce414b27-8968-4b7c-a054-5a3788ce0f94","session_id":"b9bacc24-0d88-4c7d-a678-d532362bc092"}"#;
pub const PROMOTED_UPDATED: &str = r#"{"type":"system","subtype":"task_updated","task_id":"ba28p82m4","run_id":"0mvso3an8-e7937c61","patch":{"is_backgrounded":true},"uuid":"53776972-365c-4e1b-8c4d-1012cf2a8741","session_id":"b9bacc24-0d88-4c7d-a678-d532362bc092"}"#;
pub const PROMOTED_ENDED: &str = r#"{"type":"system","subtype":"task_updated","task_id":"ba28p82m4","run_id":"0mvso3an8-e7937c61","patch":{"status":"completed","end_time":1793225502117},"uuid":"45b55761-b304-4b7e-80ae-f3c7a874ff90","session_id":"b9bacc24-0d88-4c7d-a678-d532362bc092"}"#;
pub const PROMOTED_NOTIFIED: &str = r#"{"type":"system","subtype":"task_notification","task_id":"ba28p82m4","run_id":"0mvso3an8-e7937c61","tool_use_id":"toolu_015w8zbEatreDdpozAygHZzs","status":"completed","output_file":"/tmp/claude-1000/-home-user-project/b9bacc24-0d88-4c7d-a678-d532362bc092/tasks/ba28p82m4.output","summary":"Background command \"node -e \"setTimeout(() => console.log(\\\"fg-done\\\"), 125000)\"\" completed (exit code 0)","uuid":"8eaaeb7f-2cb4-4ab4-9245-47379b5e8f61","session_id":"b9bacc24-0d88-4c7d-a678-d532362bc092"}"#;
pub const PROMOTED_RESULT: &str = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_015w8zbEatreDdpozAygHZzs","content":"Command did not complete within its 120s timeout and was moved to the background (ID: ba28p82m4)."}]},"parent_tool_use_id":null}"#;
// Captured from Claude Code 2.1.310 for a background command: the level signal joins the task
// before its own start line, and drops it after the end lines. The CLI sends the level
// whenever the set of live tasks changes, so it is also sent for a task whose end line is lost.
pub const BG_SHELL_LEVEL: &str = r#"{"type":"system","subtype":"background_tasks_changed","tasks":[{"task_id":"bad1lsfze","run_id":"0mvsvvtva-f5c9405c","task_type":"local_bash","description":"Run sleep 4 in the background"}],"uuid":"cd68add3-6bbd-46c5-88db-3e94d095aca5","session_id":"a0a1cc63-46a0-4b3b-a654-2404b1179d93"}"#;
pub const BG_SHELL_LEVEL_STARTED: &str = r#"{"type":"system","subtype":"task_started","task_id":"bad1lsfze","run_id":"0mvsvvtva-f5c9405c","tool_use_id":"toolu_01MNY4r6LKPHXpL4HiGR2xUy","description":"Run sleep 4 in the background","is_backgrounded":true,"task_type":"local_bash","uuid":"e60c84ae-357c-497d-a7b0-db2e10014829","session_id":"a0a1cc63-46a0-4b3b-a654-2404b1179d93"}"#;
pub const BG_SHELL_LEVEL_EMPTY: &str = r#"{"type":"system","subtype":"background_tasks_changed","tasks":[],"uuid":"94b187f3-d096-4142-95a3-be20d1f25296","session_id":"a0a1cc63-46a0-4b3b-a654-2404b1179d93"}"#;
