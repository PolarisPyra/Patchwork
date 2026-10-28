//! The per-event cost of a streaming turn: parsing one line of the agent's output, and saving the
//! message a tool event changes. Run with `cargo bench -p patchwork-server`; profile with
//! `cargo flamegraph -p patchwork-server --bench stream -- --bench <filter>`.

use std::hint::black_box;
use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use patchwork_server::persistence::Store;
use patchwork_server::provider::{TurnSpec, parse_line};
use patchwork_server::types::{
    AccessMode, AgentKind, Message, MessageStatus, Role, StepStatus, Thread, ToolCall, new_id,
};
use serde_json::Value;

const TEXT_DELTA: &str = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"The renderer re-reads the list after each change, so "}},"session_id":"s","parent_tool_use_id":null,"uuid":"u"}"#;
const TOOL_USE: &str = r#"{"type":"assistant","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-opus-5-5","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"cargo build --workspace","description":"Build the workspace"}}],"stop_reason":null,"usage":{"input_tokens":3,"cache_creation_input_tokens":120,"cache_read_input_tokens":40000,"output_tokens":80}},"parent_tool_use_id":null,"session_id":"s"}"#;

fn tool_result() -> String {
    let output = "   Compiling patchwork-server v0.10.8\n".repeat(60);
    serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "toolu_1", "content": output, "is_error": false}
        ]},
        "parent_tool_use_id": null,
        "session_id": "s"
    })
    .to_string()
}

fn parse(c: &mut Criterion) {
    let spec = TurnSpec {
        root: "/home/user/project",
        session_id: Some("s"),
        model: None,
        text: "",
        mode: AccessMode::Auto,
        plan: false,
        effort: None,
        attachments: &[],
        images: &[],
        instructions: None,
        skills_dir: None,
        guarded: &[],
        agents: &[],
    };
    let result = tool_result();
    let mut group = c.benchmark_group("parse_line");
    for (name, line) in [
        ("text_delta", TEXT_DELTA),
        ("tool_use", TOOL_USE),
        ("tool_result", result.as_str()),
    ] {
        group.bench_function(name, |b| {
            b.iter(|| {
                let v: Value = serde_json::from_str(black_box(line)).unwrap();
                parse_line(AgentKind::Claude, &v, &spec, "m")
            })
        });
    }
    group.finish();
}

fn tool(i: usize) -> ToolCall {
    ToolCall {
        id: format!("toolu_{i}"),
        name: "Bash".into(),
        summary: "cargo build --workspace".into(),
        status: StepStatus::Done,
        subagent: None,
        parent: None,
        started_at: Some(1),
        ended_at: Some(2),
        output: Some("   Compiling patchwork-server v0.10.8\n".repeat(50)),
        child_thread_id: None,
    }
}

fn thread(tools: usize) -> Thread {
    let message = Message {
        id: "reply".into(),
        role: Role::Assistant,
        text: "Looked at the renderer.".into(),
        created_at: 1,
        status: MessageStatus::Streaming,
        tools: (0..tools).map(tool).collect(),
        plan: vec![],
        files: vec![],
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
    };
    Thread {
        id: new_id(),
        project_id: None,
        title: "Bench".into(),
        agent: AgentKind::Claude,
        model: None,
        session_id: None,
        access_mode: AccessMode::Auto,
        created_at: 1,
        messages: vec![message],
        goal: None,
        position: 0,
        folder_id: None,
        effort: None,
        archived_at: None,
        parent: None,
        plan_mode: false,
        slash_commands: vec![],
        imported_from: None,
        context_pending: false,
        role: None,
        role_pending: false,
    }
}

/// A tool finishing in a reply that already holds `n` tool calls: what `publish` saves per event.
fn save(c: &mut Criterion) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bench-store");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(dir.clone()).unwrap();
    let mut group = c.benchmark_group("save_tool_event");
    for n in [10, 100, 400] {
        let thread = thread(n);
        let id = thread.id.clone();
        store.insert_threads(vec![thread]).unwrap();
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let mut i = 0;
            b.iter(|| {
                i += 1;
                store
                    .update_message(&id, Some("reply"), |t| {
                        t.messages[0].tools[i % n].ended_at = Some(i as u64);
                    })
                    .unwrap()
            })
        });
    }
    group.finish();
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

criterion_group!(benches, parse, save);
criterion_main!(benches);
