//! Tests for finding the files a turn changed.

// Rust guideline compliant 2026-10-09

use std::io::BufRead;
use crate::threads::AccessMode;
use std::sync::Mutex;
use crate::orchestration::turn::process::AgentStdin;
use crate::orchestration::turn::process::Stdin;
use std::process::Stdio;
use crate::provider::TurnSpec;
use std::collections::HashMap;
use std::io::BufReader;
use std::sync::Arc;

use serde_json::Value;

use crate::orchestration::changes::attribution::changed_paths;
use crate::orchestration::turn::process::write_line;
use crate::provider::{self, AgentKind, Parsed};
use crate::system::new_id;
use crate::workspace::{self, FileChange};

pub(in crate::orchestration) fn change(path: &str, status: &str, additions: u32) -> FileChange {
    FileChange {
        path: path.into(),
        status: status.into(),
        additions,
        deletions: 0,
        staged: false,
    }
}

#[test]
pub(in crate::orchestration) fn changed_paths_finds_new_and_modified_files() {
    let before = [change("a.rs", "modified", 1), change("b.rs", "modified", 2)];
    let after = [
        change("a.rs", "modified", 1),
        change("b.rs", "modified", 5),
        change("c.rs", "untracked", 3),
    ];
    assert_eq!(changed_paths(&before, &after), ["b.rs", "c.rs"]);
}

/// Runs a real turn through the installed CLI, allowing every approval, and
/// checks the parsed stream. Needs the agent installed and signed in:
/// `cargo test -- --ignored`.
pub(in crate::orchestration) fn live_turn(agent: AgentKind, mode: AccessMode) {
    let root = std::env::temp_dir().join(new_id());
    std::fs::create_dir_all(&root).unwrap();
    let root_str = root.to_str().unwrap();
    let ok = crate::system::host_command("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(ok.success());

    let spec = TurnSpec {
        root: root_str,
        host: None,
        thread_id: "thread",
        session_id: None,
        model: (agent == AgentKind::Claude).then_some("haiku"),
        vision_model: None,
        text: "Create a file named hello.txt whose only content is the two letters hi (no period). Do nothing else.",
        mode,
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
    let baseline = workspace::git_changes(root_str).unwrap();
    let input = provider::turn_input(agent, &spec);
    let mut child = provider::agent_command(agent)
        .unwrap()
        .args(provider::turn_args(agent, &spec))
        .current_dir(&root)
        .stdin(if input.is_empty() {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin: Stdin = Arc::new(Mutex::new(AgentStdin::take(&mut child, agent, "test")));
    for line in &input {
        write_line(&stdin, line);
    }

    let mut parsed = Vec::new();
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let Ok(value) = serde_json::from_str::<Value>(&line.unwrap()) else {
            continue;
        };
        for item in provider::parse_line(agent, &value, &spec, "m") {
            match &item {
                Parsed::Send(line) => {
                    write_line(&stdin, line);
                }
                Parsed::Approval { reply, .. } => {
                    write_line(
                        &stdin,
                        &provider::approval_reply(agent, reply, true, &HashMap::new()),
                    );
                }
                Parsed::Finished => drop(stdin.lock().unwrap().take()),
                _ => {}
            }
            parsed.push(item);
        }
    }
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(parsed.iter().any(|p| matches!(p, Parsed::SessionId(_))));
    assert!(
        !parsed.iter().any(|p| matches!(p, Parsed::Error(_))),
        "{parsed:?}"
    );
    if mode == AccessMode::Supervised && agent != AgentKind::Antigravity {
        assert!(parsed.iter().any(|p| matches!(p, Parsed::Approval { .. })));
    }
    let changed = changed_paths(&baseline, &workspace::git_changes(root_str).unwrap());
    assert_eq!(changed, ["hello.txt"]);
    let content = std::fs::read_to_string(root.join("hello.txt")).unwrap();
    assert_eq!(content.trim(), "hi");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "runs the real Claude Code CLI"]
pub(in crate::orchestration) fn live_claude_supervised_turn() {
    live_turn(AgentKind::Claude, AccessMode::Supervised);
}

#[test]
#[ignore = "runs the real Codex CLI"]
pub(in crate::orchestration) fn live_codex_supervised_turn() {
    live_turn(AgentKind::Codex, AccessMode::Supervised);
}

#[test]
#[ignore = "runs the real Claude Code CLI"]
pub(in crate::orchestration) fn live_claude_turn() {
    live_turn(AgentKind::Claude, AccessMode::FullAccess);
}

#[test]
#[ignore = "runs the real Codex CLI"]
pub(in crate::orchestration) fn live_codex_turn() {
    live_turn(AgentKind::Codex, AccessMode::FullAccess);
}

#[test]
#[ignore = "runs the real Antigravity CLI"]
pub(in crate::orchestration) fn live_antigravity_turn() {
    live_turn(AgentKind::Antigravity, AccessMode::AutoAcceptEdits);
}
