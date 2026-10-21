//! Agent CLIs: locating and probing them, building turn arguments, and mapping
//! their JSONL output. Each provider module knows one CLI.

pub mod claude;
pub mod codex;
pub mod antigravity;
pub mod model_manifest;

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use serde_json::Value;

use crate::types::*;
use crate::workspace::signal_process_group;

/// Upper bound for any single agent CLI probe (`--version`, `auth status`, ...).
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Directories searched when `which` cannot find the agent CLI on PATH.
fn fallback_dirs() -> Vec<PathBuf> {
    let home = home_dir().unwrap_or_default();
    let mut dirs: Vec<PathBuf> = [".local/bin", ".cargo/bin", ".bun/bin"]
        .iter()
        .map(|d| home.join(d))
        .collect();
    dirs.extend(["/usr/local/bin", "/opt/homebrew/bin"].map(PathBuf::from));
    if let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) {
        let mut nvm: Vec<PathBuf> = entries.flatten().map(|e| e.path().join("bin")).collect();
        nvm.sort();
        dirs.extend(nvm);
    }
    dirs
}

fn find_binary(name: &str) -> Option<PathBuf> {
    which::which(name).ok().or_else(|| {
        fallback_dirs()
            .into_iter()
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

/// Resolves the agent CLI (searching PATH plus common install dirs, since GUI
/// launches may not inherit the shell PATH) and returns a Command for it with
/// the binary's directory prepended to PATH (codex needs `node` beside it).
pub fn agent_command(kind: AgentKind) -> Result<Command, String> {
    let bin = match kind {
        AgentKind::Claude => find_binary("claude").ok_or("Claude Code is not installed")?,
        AgentKind::Codex => find_binary("codex").ok_or("Codex is not installed")?,
        AgentKind::Antigravity => {
            antigravity::antigravity_bin().ok_or("Antigravity CLI is not installed")?
        }
    };
    let mut cmd = Command::new(&bin);
    if let Some(dir) = bin.parent() {
        let mut paths = vec![dir.to_path_buf()];
        if let Some(existing) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        if let Ok(joined) = std::env::join_paths(paths) {
            cmd.env("PATH", joined);
        }
    }
    Ok(cmd)
}

pub struct Probe {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs the agent CLI with `args`. Returns None if it cannot start or exceeds
/// `PROBE_TIMEOUT` (the process group is killed in that case).
pub fn probe(kind: AgentKind, args: &[&str]) -> Option<Probe> {
    probe_until(kind, args, |_| false)
}

/// Like `probe`, but also ends the process once `done` accepts the stdout so far, and returns
/// that output as a success. Some CLIs print their answer and then keep running, so waiting
/// for them to exit would only ever reach the timeout.
pub fn probe_until(kind: AgentKind, args: &[&str], done: impl Fn(&str) -> bool) -> Option<Probe> {
    let mut cmd = agent_command(kind).ok()?;
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().ok()?;
    let (out, out_reader) = read_pipe(child.stdout.take()?);
    let (err, err_reader) = read_pipe(child.stderr.take()?);

    let deadline = Instant::now() + PROBE_TIMEOUT;
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Kill leftover group members so a grandchild cannot hold the pipes open.
                signal_process_group(child.id(), libc::SIGKILL);
                let _ = out_reader.join();
                let _ = err_reader.join();
                break status.success();
            }
            Ok(None) if done(&text(&out)) => {
                stop(&mut child);
                break true;
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                stop(&mut child);
                return None;
            }
        }
    };
    Some(Probe {
        success,
        stdout: text(&out),
        stderr: text(&err),
    })
}

/// Kills the CLI and anything it started in its process group.
fn stop(child: &mut Child) {
    signal_process_group(child.id(), libc::SIGKILL);
    let _ = child.kill();
    let _ = child.wait();
}

/// Copies a pipe into a shared buffer on its own thread, so a probe can read what has arrived
/// without waiting for the pipe to close.
fn read_pipe(mut pipe: impl Read + Send + 'static) -> (Arc<Mutex<Vec<u8>>>, JoinHandle<()>) {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&buf);
    let handle = std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            sink.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend_from_slice(&chunk[..n]);
        }
    });
    (buf, handle)
}

fn text(buf: &Mutex<Vec<u8>>) -> String {
    let bytes = buf.lock().unwrap_or_else(PoisonError::into_inner);
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn detect_agent(kind: AgentKind) -> AgentStatus {
    let mut status = AgentStatus {
        kind,
        installed: false,
        version: None,
        signed_in: false,
        detail: "Not installed".into(),
    };
    let Some(version) = probe(kind, &["--version"]).filter(|p| p.success) else {
        return status;
    };
    status.installed = true;
    // "2.1.303 (Claude Code)", "codex-cli 0.163.0" and "0.64.0" all carry the version as a numeric word.
    status.version = version
        .stdout
        .split_whitespace()
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()))
        .map(String::from);
    match kind {
        AgentKind::Claude => claude::auth(&mut status),
        AgentKind::Codex => codex::auth(&mut status),
        AgentKind::Antigravity => antigravity::auth(&mut status),
    }
    status
}

/// Models offered for `kind`, starting with the CLI default.
pub fn list_models(kind: AgentKind) -> Vec<ModelOption> {
    let (default_model, models) = match kind {
        AgentKind::Claude => (claude::configured_model(), claude::models()),
        AgentKind::Codex => (codex::configured_model(), codex::models()),
        AgentKind::Antigravity => (None, antigravity::models()),
    };
    let mut out = vec![ModelOption {
        id: None,
        label: "Default".into(),
        default_model: default_model.filter(|m| !m.trim().is_empty()),
    }];
    out.extend(models);
    out
}

/// What one turn runs with.
pub struct TurnSpec<'a> {
    pub root: &'a str,
    pub session_id: Option<&'a str>,
    pub model: Option<&'a str>,
    pub text: &'a str,
    pub mode: AccessMode,
    /// Plan mode: the agent plans and proposes a plan, without changing files.
    pub plan: bool,
    /// Reasoning effort, e.g. "high"; None leaves the CLI's default.
    pub effort: Option<&'a str>,
    pub attachments: &'a [Attachment],
    /// The image attachments, read from disk by `load_images`.
    pub images: &'a [Image],
}

/// An attached image, base64-encoded for the agent's input.
#[derive(Clone)]
pub struct Image {
    pub media_type: &'static str,
    pub data: String,
}

/// Largest image sent to an agent; the agents reject bigger ones.
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

/// Reads and encodes every image attachment.
pub fn load_images(attachments: &[Attachment]) -> Result<Vec<Image>, String> {
    use base64::Engine;
    attachments
        .iter()
        .filter(|a| a.kind == AttachmentKind::Image)
        .map(|a| {
            let media_type = image_media_type(&a.path).ok_or("Not an image")?;
            let size = std::fs::metadata(&a.path)
                .map_err(|e| format!("{}: {e}", a.name))?
                .len();
            if size > MAX_IMAGE_BYTES {
                return Err(format!("{} is larger than 5 MB", a.name));
            }
            let bytes = std::fs::read(&a.path).map_err(|e| format!("{}: {e}", a.name))?;
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            Ok(Image { media_type, data })
        })
        .collect()
}

/// The prompt the agent reads: the message, then the paths of what was attached,
/// so the agent can open files and folders with its own tools.
pub fn prompt(spec: &TurnSpec) -> String {
    prompt_text(spec.text, spec.attachments)
}

fn prompt_text(text: &str, attachments: &[Attachment]) -> String {
    if attachments.is_empty() {
        return text.to_string();
    }
    let lines: Vec<String> = attachments
        .iter()
        .map(|a| {
            let kind = match a.kind {
                AttachmentKind::Image => "image",
                AttachmentKind::File => "file",
                AttachmentKind::Folder => "folder",
            };
            format!("- {} ({kind})", a.path)
        })
        .collect();
    format!("{text}\n\nAttached:\n{}", lines.join("\n"))
        .trim_start()
        .to_string()
}

/// The stdin line that adds a message to a turn that is already running, or None when
/// the agent cannot take one now and the message waits for the turn to end. Antigravity reads
/// its whole prompt up front; Codex needs the turn's id, which it reports once started.
pub fn steer_input(
    agent: AgentKind,
    text: &str,
    attachments: &[Attachment],
    images: &[Image],
    session_id: Option<&str>,
    turn_id: Option<&str>,
) -> Option<String> {
    match agent {
        AgentKind::Claude => Some(claude::steer_message(
            &prompt_text(text, attachments),
            images,
        )),
        AgentKind::Codex => Some(codex::steer_input(
            session_id?,
            turn_id?,
            &prompt_text(text, attachments),
            attachments,
        )),
        AgentKind::Antigravity => None,
    }
}

/// The stdin line that pauses the agent's goal, for agents that pursue it on their own.
pub fn goal_pause(agent: AgentKind, session_id: &str) -> Option<String> {
    (agent == AgentKind::Codex).then(|| codex::goal_pause(session_id))
}

/// The stdin line that stops one subagent, for agents that can.
pub fn stop_subagent(agent: AgentKind, task_id: &str) -> Option<String> {
    (agent == AgentKind::Claude).then(|| claude::stop_task(task_id))
}

/// `/goal`, alone or with arguments, which runs as a turn of its own.
pub fn is_goal_command(text: &str) -> bool {
    text.trim_start()
        .strip_prefix("/goal")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
}

/// Command-line arguments for one turn.
pub fn turn_args(agent: AgentKind, spec: &TurnSpec) -> Vec<String> {
    match agent {
        AgentKind::Claude => claude::turn_args(spec),
        AgentKind::Codex => codex::turn_args(),
        AgentKind::Antigravity => antigravity::turn_args(spec),
    }
}

/// Lines written to the agent's stdin when the turn starts. Empty means the
/// agent takes no input, and stdin is closed.
pub fn turn_input(agent: AgentKind, spec: &TurnSpec) -> Vec<String> {
    match agent {
        AgentKind::Claude => vec![claude::user_message(&prompt(spec), spec.images)],
        AgentKind::Codex => codex::turn_input(spec),
        AgentKind::Antigravity => vec![],
    }
}

/// Maps one line of the agent's JSONL output.
pub fn parse_line(agent: AgentKind, v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
    match agent {
        AgentKind::Claude => {
            let parsed = claude::parse(v, spec.root, message_id);
            if spec.mode != AccessMode::FullAccess {
                return parsed;
            }
            // Full access still routes questions here; anything else is allowed at once.
            parsed
                .into_iter()
                .map(|p| match p {
                    Parsed::Approval {
                        reply, questions, ..
                    } if questions.is_empty() => {
                        Parsed::Send(claude::approval_reply(&reply, true, &HashMap::new()))
                    }
                    p => p,
                })
                .collect()
        }
        AgentKind::Codex => codex::parse(v, spec, message_id),
        AgentKind::Antigravity => antigravity::parse(v, spec, message_id),
    }
}

/// The stdin line answering an approval, from the `reply` the parser attached to it.
pub fn approval_reply(
    agent: AgentKind,
    reply: &Value,
    allow: bool,
    answers: &HashMap<String, Vec<String>>,
) -> String {
    match agent {
        AgentKind::Claude => claude::approval_reply(reply, allow, answers),
        AgentKind::Codex => codex::approval_reply(reply, allow, answers),
        // Antigravity runs headless and never asks.
        AgentKind::Antigravity => String::new(),
    }
}

/// What one JSONL line means for the turn.
#[derive(Debug)]
pub enum Parsed {
    Event(AgentEvent),
    SessionId(String),
    /// The agent's own id for the subagent that the tool call `tool_id` started.
    TaskStarted {
        tool_id: String,
        task_id: String,
    },
    /// A shell command the agent started itself, in the background; it runs past its tool call.
    BackgroundTask {
        tool_id: String,
        description: String,
    },
    /// That subagent ended, which for one sent to the background is after its tool call.
    TaskEnded {
        tool_id: String,
        failed: bool,
    },
    /// Subscription usage the agent reported along the way.
    Usage(Vec<UsageWindow>),
    /// Tokens one model request used. `top_level` is false for a subagent's request.
    RequestUsage {
        request_id: String,
        usage: TokenUsage,
        top_level: bool,
    },
    /// The agent's id for the turn now running, which a steering message must name.
    TurnStarted(String),
    /// The turn failed with this message.
    Error(String),
    /// Write this line to the agent's stdin.
    Send(String),
    /// The agent waits for the user; `reply` is what `approval_reply` needs to answer it.
    Approval {
        title: String,
        detail: String,
        reply: Value,
        /// Set when the agent asks the user questions instead of asking permission.
        questions: Vec<Question>,
    },
    /// The agent answered the control request with this id.
    ControlResponse(String),
    /// The agent took a steering message (Codex acknowledges each one).
    Steered,
    /// The agent refused a steering message, so it waits for the turn to end.
    SteerRejected(String),
    Goal(GoalUpdate),
    /// The turn is over; stdin is closed so the agent exits.
    Finished,
}

/// A change to the thread's `/goal`, as the agent reports it.
#[derive(Debug)]
pub enum GoalUpdate {
    /// Claude set the goal, or restated it with the checks made so far.
    Set {
        objective: String,
        checks: Option<u32>,
    },
    /// Claude's evaluator found the goal not met yet, so the agent keeps going.
    Checked,
    Cleared,
    /// Codex reports the whole goal.
    Reported(Goal),
}

/// Collapses whitespace and caps the length, for tool summaries.
fn one_line(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect()
}

/// Strips the project root so paths are project-relative; other paths pass through.
fn relative(root: &str, path: &str) -> String {
    path.strip_prefix(root)
        .and_then(|p| p.strip_prefix('/'))
        .unwrap_or(path)
        .to_string()
}

fn tool_started(
    message_id: &str,
    id: &str,
    name: &str,
    summary: String,
    subagent: Option<SubAgent>,
    parent: Option<&str>,
) -> Parsed {
    Parsed::Event(AgentEvent::ToolStarted {
        message_id: message_id.into(),
        tool: ToolCall {
            id: id.into(),
            name: name.into(),
            summary,
            status: StepStatus::Running,
            subagent,
            parent: parent.map(String::from),
            started_at: Some(now_ms()),
            ended_at: None,
            output: None,
            child_thread_id: None,
        },
    })
}

fn tool_finished(message_id: &str, tool_id: &str, is_error: bool, output: &str) -> Parsed {
    Parsed::Event(AgentEvent::ToolFinished {
        message_id: message_id.into(),
        tool_id: tool_id.into(),
        is_error,
        output: tool_output(output),
    })
}

fn thinking(message_id: &str, text: &str) -> Parsed {
    Parsed::Event(AgentEvent::Thinking {
        message_id: message_id.into(),
        text: text.into(),
        started_at: None,
        ended_at: None,
    })
}

fn file_touched(message_id: &str, path: String) -> Parsed {
    Parsed::Event(AgentEvent::FileTouched {
        message_id: message_id.into(),
        path,
    })
}

fn text_delta(message_id: &str, text: &str) -> Parsed {
    Parsed::Event(AgentEvent::TextDelta {
        message_id: message_id.into(),
        text: text.into(),
    })
}

/// One-line summary of a tool call's input: the first useful field that is present.
fn summarize(input: &Value, root: &str) -> String {
    const KEYS: [&str; 6] = [
        "command",
        "file_path",
        "pattern",
        "url",
        "description",
        "query",
    ];
    KEYS.iter()
        .find_map(|key| {
            input[*key].as_str().filter(|s| !s.is_empty()).map(|s| {
                if *key == "file_path" {
                    relative(root, s)
                } else {
                    s.to_string()
                }
            })
        })
        .map(|s| one_line(&s))
        .unwrap_or_default()
}
