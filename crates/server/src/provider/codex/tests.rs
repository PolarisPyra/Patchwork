use super::*;
use std::collections::HashMap;

use serde_json::{Value, json};

use crate::threads::AccessMode;
use crate::orchestration::AgentEvent;

use crate::threads::TaskStatus;
use crate::threads::ToolCall;

use crate::threads::StepStatus;

use crate::provider::Parsed;

use crate::provider::TurnSpec;

fn spec(session_id: Option<&'static str>, mode: AccessMode) -> TurnSpec<'static> {
    TurnSpec {
        root: "/abs",
        thread_id: "thread",
        session_id,
        model: Some("gpt"),
        vision_model: None,
        text: "go",
        mode,
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
    }
}

fn sent(out: &[Parsed]) -> Vec<Value> {
    out.iter()
        .filter_map(|p| match p {
            Parsed::Send(line) => serde_json::from_str(line).ok(),
            _ => None,
        })
        .collect()
}

#[test]
fn codex_starts_or_resumes_then_starts_the_turn() {
    let new: Vec<Value> = turn_input(&spec(None, AccessMode::FullAccess))
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(new[0]["method"], "initialize");
    assert_eq!(new[0]["params"]["capabilities"]["experimentalApi"], true);
    assert_eq!(new[1]["method"], "initialized");
    assert_eq!(new[3]["method"], "thread/start");
    assert_eq!(new[3]["params"]["cwd"], "/abs");

    let resumed = turn_input(&spec(Some("sid"), AccessMode::FullAccess));
    let resume: Value = serde_json::from_str(&resumed[3]).unwrap();
    assert_eq!(resume["method"], "thread/resume");
    assert_eq!(resume["params"]["threadId"], "sid");

    let response = json!({"id": THREAD_REQUEST, "result": {"thread": {
        "id": "t1", "model": "codex-resolved-model"
    }}});
    let mut plan_spec = spec(None, AccessMode::Supervised);
    plan_spec.model = None;
    plan_spec.plan = true;
    let out = parse(&response, &plan_spec, "m1");
    assert!(matches!(&out[0], Parsed::SessionId(id) if id == "t1"));
    let turn = &sent(&out)[0];
    assert_eq!(turn["method"], "turn/start");
    assert_eq!(turn["params"]["threadId"], "t1");
    assert_eq!(turn["params"]["input"][0]["text"], "go");
    assert_eq!(turn["params"]["approvalPolicy"], "untrusted");
    assert_eq!(turn["params"]["sandboxPolicy"]["type"], "readOnly");
    assert_eq!(turn["params"]["collaborationMode"]["mode"], "plan");
    assert_eq!(
        turn["params"]["collaborationMode"]["settings"]["model"],
        "codex-resolved-model"
    );
}

#[test]
fn codex_message_and_command() {
    let spec = spec(None, AccessMode::FullAccess);
    let msg = json!({"method": "item/completed", "params": {"item":
        {"id": "i2", "type": "agentMessage", "text": "hi"}}});
    assert!(matches!(
        parse(&msg, &spec, "m1").as_slice(),
        [Parsed::Event(AgentEvent::TextDelta { text, .. })] if text == "hi"
    ));

    let started = json!({"method": "item/started", "params": {"item": {
        "id": "i3", "type": "commandExecution", "command": "cargo  test", "status": "inProgress"}}});
    assert!(matches!(
        parse(&started, &spec, "m1").as_slice(),
        [Parsed::Event(AgentEvent::ToolStarted { tool, .. })]
            if tool.name == "Shell" && tool.summary == "cargo test"
    ));

    let done = json!({"method": "item/completed", "params": {"item": {
        "id": "i3", "type": "commandExecution", "command": "cargo test",
        "exitCode": 1, "status": "failed"}}});
    assert!(matches!(
        parse(&done, &spec, "m1").last(),
        Some(Parsed::Event(AgentEvent::ToolFinished { tool_id, is_error: true, .. })) if tool_id == "i3"
    ));
}

#[test]
fn an_open_command_or_subagent_is_a_background_task_without_a_stop() {
    let spec = spec(None, AccessMode::FullAccess);
    let parsed = parse(&serde_json::from_str(COMMAND_STARTED).unwrap(), &spec, "m1");
    let [Parsed::Event(AgentEvent::ToolStarted { tool, .. })] = parsed.as_slice() else {
        panic!("expected the command to start");
    };
    let task = background_task(tool).expect("an open command is a background task");
    assert_eq!(task.tool_id, "exec-d7e8f3a5-1046-4c24-9b3b-f176ac51ae9c");
    assert_eq!(task.kind, "command");
    assert_eq!(
        task.description,
        "/usr/bin/zsh -lc 'node -e \"setTimeout(()=>console.log(1),60000)\"'"
    );
    assert_eq!(task.status, TaskStatus::Running);
    assert_eq!(task.task_id, None);

    // A subagent's thread is read when it starts, and the reply to that read is the call that
    // shows it. This capture has no agentRole, so it is named after its path, /root/reply_ok.
    let started = parse(
        &serde_json::from_str(SUBAGENT_STARTED).unwrap(),
        &spec,
        "m1",
    );
    assert!(matches!(started.as_slice(), [Parsed::Send(line)] if line.contains("thread/read")));
    let parsed = parse(&serde_json::from_str(SUBAGENT_READ).unwrap(), &spec, "m1");
    let [Parsed::Event(AgentEvent::ToolStarted { tool, .. })] = parsed.as_slice() else {
        panic!("expected the subagent to show");
    };
    let task = background_task(tool).expect("an open subagent is a background task");
    assert_eq!(
        (task.tool_id.as_str(), task.kind.as_str()),
        ("01a1849e-06c4-77d2-a568-f48ab24548e0", "agent")
    );
    assert_eq!(task.description, "reply-ok");
    // The subagent ends as a status item and then its completion; only the completion ends the call.
    let ended = parse(
        &serde_json::from_str(SUBAGENT_COMPLETED).unwrap(),
        &spec,
        "m1",
    );
    assert!(ended.is_empty());
    let ended = parse(&serde_json::from_str(SUBAGENT_ENDED).unwrap(), &spec, "m1");
    assert!(matches!(
        ended.as_slice(),
        [Parsed::Event(AgentEvent::ToolFinished { tool_id, is_error: false, .. })]
            if tool_id == "01a1849e-06c4-77d2-a568-f48ab24548e0"
    ));

    // Other calls are not background tasks.
    let edit = ToolCall {
        id: "i5".into(),
        name: "Edit".into(),
        summary: "a.rs".into(),
        status: StepStatus::Running,
        subagent: None,
        parent: None,
        child_thread_id: None,
        started_at: None,
        ended_at: None,
        output: None,
    };
    assert!(background_task(&edit).is_none());
}

#[test]
fn codex_file_change_and_plan() {
    let spec = spec(None, AccessMode::FullAccess);
    let change = json!({"method": "item/completed", "params": {"item": {
        "id": "i4", "type": "fileChange", "status": "completed",
        "changes": [{"path": "/abs/src/a.ts", "kind": {"type": "update"}, "diff": ""}]}}});
    let out = parse(&change, &spec, "m1");
    assert!(out.iter().any(
        |p| matches!(p, Parsed::Event(AgentEvent::FileTouched { path, .. }) if path == "src/a.ts")
    ));

    let plan = json!({"method": "turn/plan/updated", "params": {"plan": [
        {"step": "a", "status": "completed"},
        {"step": "b", "status": "inProgress"},
        {"step": "c", "status": "pending"}]}});
    let out = parse(&plan, &spec, "m1");
    let [Parsed::Event(AgentEvent::PlanUpdated { steps, .. })] = out.as_slice() else {
        panic!("no plan");
    };
    let statuses: Vec<_> = steps.iter().map(|s| s.status).collect();
    assert_eq!(
        statuses,
        [StepStatus::Done, StepStatus::Running, StepStatus::Pending]
    );
}

#[test]
fn codex_approvals_and_unsupported_requests() {
    let spec = spec(None, AccessMode::Supervised);
    let request = json!({"id": 7, "method": "item/commandExecution/requestApproval",
        "params": {"itemId": "i1", "threadId": "t", "turnId": "u", "startedAtMs": 0,
            "command": "rm -rf build"}});
    let out = parse(&request, &spec, "m1");
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
    let answer: Value =
        serde_json::from_str(&approval_reply(reply, false, &HashMap::new())).unwrap();
    assert_eq!(answer, json!({"id": 7, "result": {"decision": "decline"}}));

    let input = json!({"id": 8, "method": "item/tool/requestUserInput", "params": {}});
    assert_eq!(
        sent(&parse(&input, &spec, "m1"))[0]["error"]["code"],
        -32601
    );
}

#[test]
fn codex_turn_end_and_errors() {
    let spec = spec(None, AccessMode::FullAccess);
    let retrying =
        json!({"method": "error", "params": {"error": {"message": "x"}, "willRetry": true}});
    assert!(parse(&retrying, &spec, "m1").is_empty());

    let failed = json!({"method": "turn/completed", "params": {"turn": {
        "status": "failed", "error": {"message": "usage limit"}}}});
    assert!(matches!(
        parse(&failed, &spec, "m1").as_slice(),
        [Parsed::Error(e), Parsed::Finished] if e == "usage limit"
    ));

    let rejected = json!({"id": 3, "error": {"code": -1, "message": "bad model"}});
    assert!(matches!(
        parse(&rejected, &spec, "m1").as_slice(),
        [Parsed::Error(e), Parsed::Finished] if e == "bad model"
    ));
}

// Lines the app-server sent, captured from Codex 0.165.0 in three turns: a quick command and a
// command the model left running when its turn completed; then a subagent that replied and was
// waited for, with its thread read. Verbatim, but the capture folder in paths is shortened to
// /abs/work.
pub const QUICK_STARTED: &str = r#"{"method":"item/started","params":{"item":{"type":"commandExecution","id":"exec-348cbccd-c6ef-4806-8a52-da299ad1e0c6","pluginId":null,"scriptPath":null,"command":"/usr/bin/zsh -lc 'echo quick'","cwd":"/abs/work","processId":"90211","source":"unifiedExecStartup","status":"inProgress","commandActions":[{"type":"unknown","command":"echo quick"}],"aggregatedOutput":null,"exitCode":null,"durationMs":null},"threadId":"01a18499-c0de-7941-9fa3-1d4e1ac33199","turnId":"01a18499-c2cd-75e1-96c9-3f1fcd924d6f","startedAtMs":1793226036711},"emittedAtMs":1793226036711}"#;
pub const QUICK_COMPLETED: &str = r#"{"method":"item/completed","params":{"item":{"type":"commandExecution","id":"exec-348cbccd-c6ef-4806-8a52-da299ad1e0c6","pluginId":null,"scriptPath":null,"command":"/usr/bin/zsh -lc 'echo quick'","cwd":"/abs/work","processId":"90211","source":"unifiedExecStartup","status":"completed","commandActions":[{"type":"unknown","command":"echo quick"}],"aggregatedOutput":"quick\n","exitCode":0,"durationMs":0},"threadId":"01a18499-c0de-7941-9fa3-1d4e1ac33199","turnId":"01a18499-c2cd-75e1-96c9-3f1fcd924d6f","completedAtMs":1793226036711},"emittedAtMs":1793226036712}"#;
pub const COMMAND_STARTED: &str = r#"{"method":"item/started","params":{"item":{"type":"commandExecution","id":"exec-d7e8f3a5-1046-4c24-9b3b-f176ac51ae9c","pluginId":null,"scriptPath":null,"command":"/usr/bin/zsh -lc 'node -e \"setTimeout(()=>console.log(1),60000)\"'","cwd":"/abs/work","processId":"33942","source":"unifiedExecStartup","status":"inProgress","commandActions":[{"type":"unknown","command":"node -e \"setTimeout(()=>console.log(1),60000)\""}],"aggregatedOutput":null,"exitCode":null,"durationMs":null},"threadId":"01a18499-c0de-7941-9fa3-1d4e1ac33199","turnId":"01a18499-c2cd-75e1-96c9-3f1fcd924d6f","startedAtMs":1793226039092},"emittedAtMs":1793226039092}"#;
pub const TURN_COMPLETED: &str = r#"{"method":"turn/completed","params":{"threadId":"01a18499-c0de-7941-9fa3-1d4e1ac33199","turn":{"id":"01a18499-c2cd-75e1-96c9-3f1fcd924d6f","rootTurnId":"01a18499-c2cd-75e1-96c9-3f1fcd924d6f","items":[{"type":"agentMessage","id":"msg_033c402dbb36bf0d016ae2753a66288190850b7889fcef2268","text":"started","phase":"final_answer","memoryCitation":null,"delivery":null,"questions":null}],"itemsView":"summary","status":"completed","error":null,"startedAt":1793226031,"completedAt":1793226042,"durationMs":10715}},"emittedAtMs":1793226042544}"#;
pub const SUBAGENT_STARTED: &str = r#"{"method":"item/started","params":{"item":{"type":"subAgentActivity","model":"gpt-6-astra","reasoningEffort":"medium","id":"call_f413e8be4e1344b8816f402f5b1c5590","kind":"started","agentThreadId":"01a1849e-06c4-77d2-a568-f48ab24548e0","agentPath":"/root/reply_ok"},"threadId":"01a1849d-f66f-78f2-b130-ccc4e186536a","turnId":"01a1849d-f686-76f1-afb8-a9f36d0f6e89","startedAtMs":1793226311381},"emittedAtMs":1793226311381}"#;
pub const SUBAGENT_READ: &str = r#"{"id":"subagent 01a1849e-06c4-77d2-a568-f48ab24548e0 reply_ok","result":{"thread":{"id":"01a1849e-06c4-77d2-a568-f48ab24548e0","environments":[{"environmentId":"local","cwd":"/abs/work","runtimeWorkspaceRoots":["/abs/work"]}],"extra":null,"sessionId":"01a1849d-f66f-78f2-b130-ccc4e186536a","forkedFromId":"01a1849d-f66f-78f2-b130-ccc4e186536a","parentThreadId":"01a1849d-f66f-78f2-b130-ccc4e186536a","preview":"","ephemeral":false,"section":null,"sectionEnteredAt":null,"projectId":null,"historyMode":"paginated","modelProvider":"headroom","model":"gpt-6-astra","reasoningEffort":"medium","createdAt":1793226311,"updatedAt":1793226311,"recencyAt":1793226311,"status":{"type":"idle"},"path":"/home/polaris/.codex/sessions/2026/10/28/rollout-2026-10-28T18-25-11-01a1849e-06c4-77d2-a568-f48ab24548e0.jsonl","cwd":"/abs/work","cliVersion":"0.165.0","originator":"patchwork-capture","source":{"subAgent":{"thread_spawn":{"parent_thread_id":"01a1849d-f66f-78f2-b130-ccc4e186536a","depth":1,"agent_path":"/root/reply_ok","agent_nickname":"Euler","agent_role":null}}},"canAcceptDirectInput":false,"threadSource":"subagent","agentNickname":"Euler","agentRole":null,"gitInfo":{"sha":"cf8edd47f69c5212f25f10123708afdfe5adc46f","branch":"main","originUrl":"git@github.com:PolarisPyra/Patchwork-ADE.git"},"name":null,"daybreakEnabled":null,"turns":[]},"readState":null}}"#;
pub const SUBAGENT_COMPLETED: &str = r#"{"method":"item/started","params":{"item":{"type":"subAgentActivity","model":null,"reasoningEffort":null,"id":"subagent-completed-01a1849e-06d4-7953-9208-38440b33c70b","kind":"completed","agentThreadId":"01a1849e-06c4-77d2-a568-f48ab24548e0","agentPath":"/root/reply_ok"},"threadId":"01a1849d-f66f-78f2-b130-ccc4e186536a","turnId":"01a1849d-f686-76f1-afb8-a9f36d0f6e89","startedAtMs":1793226317613},"emittedAtMs":1793226317613}"#;
pub const SUBAGENT_ENDED: &str = r#"{"method":"item/completed","params":{"item":{"type":"subAgentActivity","model":null,"reasoningEffort":null,"id":"subagent-completed-01a1849e-06d4-7953-9208-38440b33c70b","kind":"completed","agentThreadId":"01a1849e-06c4-77d2-a568-f48ab24548e0","agentPath":"/root/reply_ok"},"threadId":"01a1849d-f66f-78f2-b130-ccc4e186536a","turnId":"01a1849d-f686-76f1-afb8-a9f36d0f6e89","completedAtMs":1793226317613},"emittedAtMs":1793226317613}"#;
