//! A chat's run through the `Runner`, over stand-in agent processes: queueing behind a turn,
//! steering it, stopping it while it starts and while it runs, a permission hook waiting for the
//! user, and the run forgotten once its agent exits.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::Core;
use crate::chats::{AccessMode, ApprovalStatus, MessageStatus, NewChat};
use crate::events::ChatEvent;
use crate::orchestration::output::ingest::ingest;
use crate::orchestration::output::ingest::replays::test_turn;
use crate::orchestration::turn::start::{Starting, assistant_message};
use crate::orchestration::{ApprovalDecision, Delivery, FollowUpInput};
use crate::permissions::{Effect, PermissionKind, PermissionRule, Target};
use crate::provider::AgentKind;

/// How long a test waits for the runner to reach a state before it fails.
const PATIENCE: Duration = Duration::from_secs(20);

/// One chat with a stand-in agent process, and the thread reading its output.
struct Run {
    core: Arc<Core>,
    chat_id: String,
    message_id: String,
    root: PathBuf,
    dir: PathBuf,
    reader: Option<JoinHandle<()>>,
}

impl Run {
    /// A chat for `agent`, with no turn yet.
    fn new(agent: AgentKind) -> Self {
        let dir = std::env::temp_dir().join(format!("patchwork-run-{}", crate::system::new_id()));
        std::fs::create_dir_all(&dir).expect("the temporary folder is made");
        let core = Core::for_tests(&dir, Box::new(|_| {}));
        let chat = core
            .create_chat(NewChat {
                id: None,
                project_id: None,
                agent,
                model: None,
                access_mode: AccessMode::Supervised,
                effort: None,
                compact_window: None,
            })
            .expect("the chat is made");
        let root = dir.join("root");
        std::fs::create_dir_all(&root).expect("the root is made");
        Self {
            core,
            chat_id: chat.id,
            message_id: String::new(),
            root,
            dir,
            reader: None,
        }
    }

    /// Starts a turn whose agent runs `script` in `sh`, registered as a launch registers one.
    /// True when the chat was stopped while the turn was starting.
    fn start(&mut self, agent: AgentKind, script: &str) -> bool {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // In a group of its own, as a launch starts an agent, so a stop reaches all of it.
            .process_group(0)
            .spawn()
            .expect("sh starts");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let stderr_tail = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut { stderr }, &mut text);
            text
        });
        let message = assistant_message();
        self.message_id.clone_from(&message.id);
        let turn = test_turn(
            &self.core,
            &self.chat_id,
            agent,
            &self.root,
            &mut child,
            &message.id,
        );
        turn.publish(ChatEvent::MessageStarted { message });
        let stop = self.core.runner.register(&turn, child.id());
        self.reader = Some(std::thread::spawn(move || {
            ingest(turn, child, stdout, stderr_tail);
        }));
        stop
    }

    /// Waits until the agent has exited and its reader has ended the turn.
    fn wait_ended(&mut self) {
        let reader = self.reader.take().expect("a turn was started");
        wait_for("the turn to end", || reader.is_finished());
        reader.join().expect("ingest does not panic");
    }

    /// The status the turn's reply was saved with.
    fn reply_status(&self) -> Option<MessageStatus> {
        let chat = self.core.store.chat(&self.chat_id)?;
        chat.messages
            .iter()
            .find(|m| m.id == self.message_id)
            .map(|m| m.status)
    }

    /// The approvals still waiting in the turn's reply.
    fn pending_approvals(&self) -> Vec<String> {
        self.core
            .store
            .chat(&self.chat_id)
            .into_iter()
            .flat_map(|chat| chat.messages.clone())
            .flat_map(|m| m.approvals.clone())
            .filter(|a| a.status == ApprovalStatus::Pending)
            .map(|a| a.id)
            .collect()
    }

    /// Asserts the runner holds nothing of the chat.
    fn assert_forgotten(&self) {
        let runner = &self.core.runner;
        assert!(!runner.has_agent(&self.chat_id), "the agent is forgotten");
        assert!(!runner.is_running(&self.chat_id), "no reply runs");
        assert!(!runner.is_starting(&self.chat_id), "no turn starts");
        assert!(runner.agent_pids().is_empty(), "no agent is up");
        assert!(runner.running().is_empty(), "no chat runs");
        assert_eq!(
            runner.tracked(),
            0,
            "the runner keeps no state for the chat"
        );
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.core.runner.cancel(&self.chat_id);
        if let Some(reader) = self.reader.take() {
            let deadline = Instant::now() + PATIENCE;
            while !reader.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Waits until `done`, failing the test after `PATIENCE`.
fn wait_for(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A message of the user's with `text`.
fn input<'a>(message_id: &'a str, text: &'a str) -> FollowUpInput<'a> {
    FollowUpInput {
        message_id,
        text,
        attachments: &[],
        images: &[],
        merged: &[],
    }
}

/// A recorded Claude reply that ends cleanly, which leaves the agent idle.
fn claude_reply() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/ingest/claude.jsonl")
}

#[test]
fn queued_message_waits_for_the_running_turn() {
    let mut run = Run::new(AgentKind::Claude);
    let runner = &run.core.runner;
    assert!(
        !runner.enqueue(&run.chat_id, input("m-1", "later")),
        "nothing to queue behind"
    );
    run.start(AgentKind::Claude, "cat >/dev/null");
    let runner = &run.core.runner;
    assert!(runner.is_running(&run.chat_id));
    assert!(runner.enqueue(&run.chat_id, input("m-1", "later")));
    assert!(runner.is_queued(&run.chat_id, "m-1"));
    assert!(runner.is_handed_over(&run.chat_id, "m-1"));
    assert!(runner.edit_queued(&run.chat_id, input("m-1", "edited")));
    assert!(
        runner.withdraw(&run.chat_id, "m-1"),
        "a queued message comes back"
    );
    assert!(!runner.is_queued(&run.chat_id, "m-1"));
    assert!(!runner.is_handed_over(&run.chat_id, "m-1"));
    assert!(
        !runner.edit_queued(&run.chat_id, input("m-1", "again")),
        "a withdrawn message is not edited"
    );
    // Queued again, it is still waiting when the turn is stopped, and the turn's end hands it
    // back.
    assert!(runner.enqueue(&run.chat_id, input("m-2", "next")));
    runner.cancel(&run.chat_id);
    run.wait_ended();
    assert!(!run.core.runner.is_handed_over(&run.chat_id, "m-2"));
    assert!(run.core.runner.withdraw(&run.chat_id, "m-2"));
    run.assert_forgotten();
}

#[test]
fn steer_is_written_to_the_running_agent() {
    let mut run = Run::new(AgentKind::Claude);
    let seen = run.root.join("stdin.txt");
    let runner = &run.core.runner;
    assert!(matches!(
        runner.steer(&run.chat_id, input("m-1", "x"), None, &[]),
        Delivery::NotRunning
    ));
    run.start(AgentKind::Claude, "cat > stdin.txt");
    let runner = &run.core.runner;
    let delivery = runner.steer(&run.chat_id, input("m-1", "turn left"), None, &[]);
    assert!(matches!(delivery, Delivery::Steered), "{delivery:?}");
    assert!(runner.is_handed_over(&run.chat_id, "m-1"));
    assert!(!runner.withdraw(&run.chat_id, "m-1"), "the agent has it");
    wait_for("the steer to reach the agent", || {
        std::fs::read_to_string(&seen).is_ok_and(|text| text.contains("turn left"))
    });
    // Stopped, the turn takes nothing more: a message waits for the next turn.
    runner.cancel(&run.chat_id);
    let delivery = runner.steer(&run.chat_id, input("m-2", "too late"), None, &[]);
    assert!(
        matches!(delivery, Delivery::Queued | Delivery::NotRunning),
        "{delivery:?}"
    );
    run.wait_ended();
    // The stopped turn hands back the steer it never answered.
    assert!(!run.core.runner.is_handed_over(&run.chat_id, "m-1"));
    assert!(!run.core.runner.is_handed_over(&run.chat_id, "m-2"));
    run.assert_forgotten();
}

#[test]
fn stop_during_a_turn_ends_it_cancelled() {
    let mut run = Run::new(AgentKind::Claude);
    run.start(AgentKind::Claude, "exec sleep 600");
    let runner = &run.core.runner;
    assert!(runner.has_agent(&run.chat_id));
    assert_eq!(runner.running(), vec![run.chat_id.clone()]);
    assert_eq!(runner.agent_pids().len(), 1);
    runner.cancel(&run.chat_id);
    run.wait_ended();
    assert_eq!(run.reply_status(), Some(MessageStatus::Cancelled));
    run.assert_forgotten();
}

#[test]
fn agent_that_exits_is_forgotten() {
    let mut run = Run::new(AgentKind::Claude);
    let script = format!("cat '{}'", claude_reply().display());
    run.start(AgentKind::Claude, &script);
    run.wait_ended();
    assert_eq!(run.reply_status(), Some(MessageStatus::Complete));
    run.assert_forgotten();
}

#[test]
fn idle_agent_takes_the_next_message() {
    let mut run = Run::new(AgentKind::Claude);
    let script = format!("cat '{}'; cat > stdin.txt", claude_reply().display());
    run.start(AgentKind::Claude, &script);
    let runner = &run.core.runner;
    let chat_id = run.chat_id.clone();
    wait_for("the agent to go idle", || {
        runner.has_agent(&chat_id) && !runner.is_running(&chat_id)
    });
    assert_eq!(run.reply_status(), Some(MessageStatus::Complete));
    assert!(
        !runner.enqueue(&chat_id, input("m-1", "x")),
        "nothing to queue behind an idle agent"
    );
    let delivery = runner.steer(&chat_id, input("m-1", "next one"), None, &[]);
    assert!(matches!(delivery, Delivery::Resumed), "{delivery:?}");
    assert!(runner.is_running(&chat_id));
    let seen = run.root.join("stdin.txt");
    wait_for("the message to reach the agent", || {
        std::fs::read_to_string(&seen).is_ok_and(|text| text.contains("next one"))
    });
    runner.cancel(&chat_id);
    run.wait_ended();
    run.assert_forgotten();
}

#[test]
fn retired_idle_agent_is_forgotten() {
    let mut run = Run::new(AgentKind::Claude);
    let script = format!("cat '{}'; cat >/dev/null", claude_reply().display());
    run.start(AgentKind::Claude, &script);
    let runner = &run.core.runner;
    let chat_id = run.chat_id.clone();
    wait_for("the agent to go idle", || {
        runner.has_agent(&chat_id) && !runner.is_running(&chat_id)
    });
    runner.retire_all_idle();
    assert!(
        !runner.has_agent(&chat_id),
        "a retired agent is let go at once"
    );
    run.wait_ended();
    run.assert_forgotten();
}

#[test]
fn stop_while_starting_stops_the_agent_once_up() {
    let mut run = Run::new(AgentKind::Claude);
    let runner = Arc::clone(&run.core);
    let runner = &runner.runner;
    let claim = Starting::claim(runner, &run.chat_id).expect("the first claim holds");
    assert!(runner.is_starting(&run.chat_id));
    assert!(
        Starting::claim(runner, &run.chat_id).is_err(),
        "a second start waits"
    );
    runner.cancel(&run.chat_id);
    assert!(
        run.start(AgentKind::Claude, "exec sleep 600"),
        "the stop is passed on"
    );
    drop(claim);
    runner.cancel(&run.chat_id);
    run.wait_ended();
    run.assert_forgotten();

    // A stop asked of a start that failed is dropped with it.
    let claim = Starting::claim(runner, &run.chat_id).expect("the chat is free again");
    runner.cancel(&run.chat_id);
    drop(claim);
    assert!(!runner.is_starting(&run.chat_id));
    run.assert_forgotten();
    assert!(
        !run.start(AgentKind::Claude, "exec sleep 600"),
        "no stop is left over"
    );
    run.core.runner.cancel(&run.chat_id);
    run.wait_ended();
    run.assert_forgotten();
}

/// A permission hook's request to run `command` in `run`'s chat, which a rule asks about.
fn hook_request(run: &Run, command: &str) -> Value {
    run.core
        .store
        .update_settings(|settings| {
            settings.permissions.everywhere.rules = vec![PermissionRule {
                id: "ask".into(),
                kind: PermissionKind::Command,
                pattern: "*".into(),
                effect: Effect::Ask,
            }];
        })
        .expect("the rule is saved");
    json!({
        "chat": run.chat_id,
        "root": run.root.display().to_string(),
        "kind": "command",
        "resources": [command],
        "title": "Run command",
        "detail": command,
    })
}

/// Sends `request` to the runner's hook on a thread of its own, as the hook's server does.
fn ask_hook(run: &Run, request: Value) -> JoinHandle<Value> {
    let core = Arc::clone(&run.core);
    std::thread::spawn(move || core.runner.hook_request(&core, &request))
}

#[test]
fn hook_waits_for_the_answer_and_grants_the_call_once() {
    let mut run = Run::new(AgentKind::Claude);
    run.start(AgentKind::Claude, "cat >/dev/null");
    let asked = ask_hook(&run, hook_request(&run, "touch made.txt"));
    wait_for("the approval to be shown", || {
        !run.pending_approvals().is_empty()
    });
    let approval_id = run.pending_approvals().remove(0);
    let decision = ApprovalDecision {
        approval_id: &approval_id,
        allow: true,
        answers: HashMap::new(),
        rule: None,
    };
    run.core
        .runner
        .respond(&run.core, &run.chat_id, decision)
        .expect("the answer is taken");
    let answer = asked.join().expect("the hook does not panic");
    assert_eq!(answer, json!({"deny": null}));
    let target = Target::new(PermissionKind::Command, ["touch made.txt"]);
    let runner = &run.core.runner;
    assert!(runner.take_grant(&run.chat_id, &target, &run.root));
    assert!(
        !runner.take_grant(&run.chat_id, &target, &run.root),
        "a grant counts once"
    );
    runner.cancel(&run.chat_id);
    run.wait_ended();
    run.assert_forgotten();
}

#[test]
fn hook_declined_refuses_the_call() {
    let mut run = Run::new(AgentKind::Claude);
    run.start(AgentKind::Claude, "cat >/dev/null");
    let asked = ask_hook(&run, hook_request(&run, "touch made.txt"));
    wait_for("the approval to be shown", || {
        !run.pending_approvals().is_empty()
    });
    let approval_id = run.pending_approvals().remove(0);
    let decision = ApprovalDecision {
        approval_id: &approval_id,
        allow: false,
        answers: HashMap::new(),
        rule: None,
    };
    run.core
        .runner
        .respond(&run.core, &run.chat_id, decision)
        .expect("the answer is taken");
    let answer = asked.join().expect("the hook does not panic");
    assert!(answer["deny"].is_string(), "{answer}");
    let target = Target::new(PermissionKind::Command, ["touch made.txt"]);
    assert!(!run.core.runner.take_grant(&run.chat_id, &target, &run.root));
    run.core.runner.cancel(&run.chat_id);
    run.wait_ended();
    run.assert_forgotten();
}

#[test]
fn hook_waiting_when_the_turn_stops_is_refused() {
    let mut run = Run::new(AgentKind::Claude);
    run.start(AgentKind::Claude, "cat >/dev/null");
    let asked = ask_hook(&run, hook_request(&run, "touch made.txt"));
    wait_for("the approval to be shown", || {
        !run.pending_approvals().is_empty()
    });
    run.core.runner.cancel(&run.chat_id);
    let answer = asked.join().expect("the hook does not panic");
    assert!(answer["deny"].is_string(), "{answer}");
    run.wait_ended();
    run.assert_forgotten();
}

#[test]
fn hook_without_a_turn_is_refused() {
    let run = Run::new(AgentKind::Claude);
    let answer = run
        .core
        .runner
        .hook_request(&run.core, &hook_request(&run, "touch made.txt"));
    assert!(answer["deny"].is_string(), "{answer}");
    run.assert_forgotten();
}
