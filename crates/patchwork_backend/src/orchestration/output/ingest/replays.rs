//! Recorded agent output replayed through `ingest`, with the events it publishes compared to
//! saved snapshots. Set `PATCHWORK_UPDATE_SNAPSHOTS=1` to write the snapshots anew.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use super::{Wake, ingest};
use crate::Core;
use crate::chats::{AccessMode, NewChat};
use crate::orchestration::changes::attribution::Attribution;
use crate::events::{ChatEvent, ToWindow};
use crate::orchestration::turn::process::{AgentStdin, Stdin};
use crate::orchestration::turn::start::{Turn, assistant_message};
use crate::provider::AgentKind;
use crate::system::Locked;

/// Where the recorded lines and their snapshots are kept.
fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/ingest")
}

/// A folder of its own for one replay.
fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("patchwork-replay-{}", crate::system::new_id()));
    std::fs::create_dir_all(&dir).expect("the temporary folder is made");
    dir
}

/// Replaces ids and times with stable stand-ins, and sorts keys.
fn mask(value: &mut Value, ids: &mut HashMap<String, String>, key: Option<&str>) {
    match value {
        Value::String(text) if uuid::Uuid::parse_str(text).is_ok() => {
            let next = format!("<id-{}>", ids.len() + 1);
            *text = ids.entry(text.clone()).or_insert(next).clone();
        }
        Value::Number(_)
            if key.is_some_and(|k| {
                k.ends_with("At") || k.ends_with("Ms") || k == "elapsed" || k == "timestamp"
            }) =>
        {
            *value = Value::from(0);
        }
        Value::Array(items) => items.iter_mut().for_each(|item| mask(item, ids, None)),
        Value::Object(map) => {
            // Key order depends on serde_json's features, which differ across the workspace.
            map.sort_keys();
            for (k, v) in map.iter_mut() {
                mask(v, ids, Some(k));
            }
        }
        _ => {}
    }
}

/// A turn of `agent` in `chat_id` over the process `child`, as a launch makes one, writing
/// into the reply `message_id`.
pub(in crate::orchestration) fn test_turn(
    core: &Arc<Core>,
    chat_id: &str,
    agent: AgentKind,
    root: &Path,
    child: &mut std::process::Child,
    message_id: &str,
) -> Turn {
    let stdin: Stdin = Arc::new(Mutex::new(AgentStdin::take(child, agent, chat_id)));
    Turn {
        core: Arc::clone(core),
        chat_id: chat_id.to_owned(),
        message_id: message_id.to_owned(),
        agent,
        cancelled: Arc::default(),
        interrupts: Arc::default(),
        exited: Arc::default(),
        attribution: Attribution::shared(Vec::new()),
        scope: None,
        stdin,
        pending: Arc::default(),
        native: Arc::default(),
        steers: Arc::default(),
        queue: Arc::default(),
        controls: Arc::default(),
        idle: Arc::new(AtomicBool::new(false)),
        alone: Arc::new(AtomicBool::new(false)),
        compacting: Arc::new(AtomicBool::new(false)),
        cancelling: Arc::new(AtomicBool::new(false)),
        wake: Wake::default(),
        last_output: Arc::new(AtomicU64::new(0)),
        calls_open: Arc::new(AtomicBool::new(false)),
        stalled: Arc::new(AtomicBool::new(false)),
        root: root.display().to_string(),
        session_id: None,
        model: None,
        vision_model: None,
        text: "Hello".into(),
        resumes: false,
        sent_context: false,
        mode: Arc::new(Mutex::new(AccessMode::Supervised)),
        effort: None,
        compact_window: Arc::new(AtomicU64::new(0)),
        settings: Arc::default(),
        unlimited_steps: true,
        token_limit: 0,
        attachments: Vec::new(),
        images: Vec::new(),
        instructions: None,
        skills_dir: None,
        guarded: Vec::new(),
        agents: Vec::new(),
        hooks: Vec::new(),
        dispatcher: false,
    }
}

/// Runs `lines` through `ingest` as `agent`'s output and returns what it published.
fn replay(agent: AgentKind, model: Option<&str>, lines: &Path) -> String {
    let dir = temp_dir();
    let events: Arc<Mutex<Vec<Value>>> = Arc::default();
    let seen = Arc::clone(&events);
    let core = Core::for_tests(
        &dir,
        Box::new(move |sent| {
            if let ToWindow::Event(envelope) = sent {
                let value = serde_json::to_value(&envelope.event).expect("events serialize");
                seen.locked().push(value);
            }
        }),
    );
    let chat = core
        .create_chat(NewChat {
            id: None,
            project_id: None,
            agent,
            model: model.map(str::to_owned),
            access_mode: AccessMode::Supervised,
            effort: None,
            compact_window: None,
        })
        .expect("the chat is made");
    let root = dir.join("root");
    std::fs::create_dir_all(&root).expect("the root is made");
    let mut child = Command::new("cat")
        .arg(lines)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cat starts");
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let stderr_tail = std::thread::spawn(move || {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut { stderr }, &mut text).expect("stderr reads");
        text
    });
    let message = assistant_message();
    let mut turn = test_turn(&core, &chat.id, agent, &root, &mut child, &message.id);
    turn.model = model.map(str::to_owned);
    turn.settings.locked().model = model.map(str::to_owned);
    turn.publish(ChatEvent::MessageStarted { message });
    let reader = std::thread::spawn(move || ingest(turn, child, stdout, stderr_tail));
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !reader.is_finished() {
        assert!(
            std::time::Instant::now() < deadline,
            "the replay of {} did not end",
            lines.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    reader.join().expect("ingest does not panic");
    let mut ids = HashMap::new();
    ids.insert(chat.id.clone(), "<chat>".to_owned());
    let root_text = root.display().to_string();
    let mut out = String::new();
    for mut event in events.locked().drain(..) {
        mask(&mut event, &mut ids, None);
        let line = event.to_string().replace(&root_text, "<root>");
        out.push_str(&line);
        out.push('\n');
    }
    drop(core);
    let _ = std::fs::remove_dir_all(&dir);
    out
}

/// Replays `name`.jsonl as `agent` and compares it with `name`.`snapshot`.
fn check(agent: AgentKind, model: Option<&str>, name: &str, snapshot: &str) {
    let lines = testdata().join(format!("{name}.jsonl"));
    let got = replay(agent, model, &lines);
    let path = testdata().join(format!("{snapshot}.snap"));
    if std::env::var_os("PATCHWORK_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &got).expect("the snapshot is written");
        return;
    }
    let want = std::fs::read_to_string(&path).expect("the snapshot exists");
    assert_eq!(got, want, "{snapshot} changed");
}

#[test]
fn claude() {
    check(AgentKind::Claude, None, "claude", "claude");
}

#[test]
fn codex() {
    check(AgentKind::Codex, Some("gpt-6-luna"), "codex", "codex");
}

#[test]
fn antigravity() {
    check(AgentKind::Antigravity, None, "acp", "antigravity");
}

#[test]
fn qwen() {
    check(AgentKind::Qwen, None, "acp", "qwen");
}

#[test]
fn api() {
    check(AgentKind::Api, None, "api", "api");
}
