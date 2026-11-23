//! agy's swarms: the workers a coordinator adds, read from the files agy keeps, and how the
//! swarm's tool calls are shown.
//!
//! agy sends nothing about a swarm over ACP. Its roster is `members.json` in the coordinator's
//! `swarm` folder, and each worker's steps are in its own conversation's transcript, so both are
//! reread on a timer while a swarm runs. A worker's state comes from its transcript, as agy's own
//! swarm board decides it: the roster's `wake_mode` is only a fallback.

// Rust guideline compliant 2026-11-23

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use crate::chats::{BackgroundTask, SubAgent, TaskStatus};
use crate::provider::{Parsed, TaskEvent, ToolStart, one_line, tool_finished, tool_started};
use crate::system::now_ms;

use super::children::{Transcript, conversation_dir, is_conversation_id, transcript_path};

/// Prefix of the ids of the calls that stand for swarm workers (`swarm:<conversationId>`).
pub const SWARM_TASK_PREFIX: &str = "swarm:";

/// How often a running swarm's roster and its workers' transcripts are reread.
pub const SWARM_POLL: Duration = Duration::from_secs(2);

/// The background task listed while workers are at work. It keeps the agent's input open, so a
/// reply that would otherwise close it does not end the swarm under them, and keeps an idle
/// coordinator from being retired or a quiet one from being taken for stalled.
const HOLD_TASK: &str = "swarm:hold";

/// A working worker whose transcript has not moved for this long is stuck, as agy's board
/// reports it, and no longer holds the swarm open.
const STUCK: Duration = Duration::from_secs(10 * 60);

/// The most of a transcript's end read to decide whether its worker's turn has ended. A single
/// step longer than this reads as mid-turn until the next step is written.
const MAX_TAIL_BYTES: u64 = 256 * 1024;

/// The largest `members.json` read (4 MiB).
const MAX_MEMBERS_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Upper bound on batch swarm worker launches parsed from untrusted tool input.
const MAX_SWARM_ADD_LAUNCHES: usize = 64;

/// One reply's view of agy's swarm.
#[derive(Debug, Default)]
pub struct Swarm {
    /// The coordinator called a swarm tool, so its roster is reread.
    running: bool,
    /// The hold task is open.
    held: bool,
    /// Workers by conversation id.
    workers: HashMap<String, Worker>,
}

/// A worker on the roster.
#[derive(Debug)]
struct Worker {
    /// Set once the swarm kills it, which ends its chat.
    killed: Arc<AtomicBool>,
    /// A monitor or narrator, whose periodic pulses do not hold the swarm open.
    housekeeping: bool,
    /// When it was first seen, for a worker that never writes a transcript.
    seen_at: Instant,
    /// The transcript's length and change time when `ended` was last decided.
    read: Option<(u64, SystemTime)>,
    /// Its transcript's newest step ends a turn.
    ended: bool,
}

impl Swarm {
    /// Notes a call of tool `name`; a swarm tool means the swarm's files are worth reading.
    pub fn note_tool(&mut self, name: &str) {
        if matches!(name, "swarm" | "post_message" | "read_board" | "swarm_wait") {
            self.running = true;
        }
    }

    /// How long to wait for output before rereading the swarm, while one runs.
    #[must_use]
    pub fn poll_every(&self) -> Option<Duration> {
        self.running.then_some(SWARM_POLL)
    }

    /// Whether workers are at work, so the agent's input stays open.
    #[must_use]
    pub fn is_holding(&self) -> bool {
        self.held
    }

    /// The transcript the chat of the worker call `tool_id` stands for follows.
    #[must_use]
    pub fn transcript(&self, tool_id: &str) -> Option<Transcript> {
        let id = tool_id.strip_prefix(SWARM_TASK_PREFIX)?;
        let worker = self.workers.get(id)?;
        Transcript::worker(id, Arc::clone(&worker.killed))
    }

    /// Rereads coordinator `session_id`'s roster: a call under `message_id` for each worker
    /// added, an end to the chat of each one killed, and the hold opened or closed.
    pub fn poll(&mut self, session_id: &str, message_id: &str) -> Vec<Parsed> {
        if !self.running {
            return Vec::new();
        }
        let Some(roster) = read_roster(session_id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut working = false;
        for (name, member) in roster["members"].as_object().into_iter().flatten() {
            let Some(id) = member["conversation_id"]
                .as_str()
                .filter(|id| is_conversation_id(id))
            else {
                continue;
            };
            let role = role_of(name, member);
            if role == "coordinator" {
                continue;
            }
            let killed = member["killed"].as_bool() == Some(true);
            let worker = match self.workers.entry(id.into()) {
                Entry::Occupied(entry) => entry.into_mut(),
                // One killed before it was seen has nothing to show.
                Entry::Vacant(_) if killed => continue,
                Entry::Vacant(entry) => {
                    out.extend(added(id, name, &role, member, message_id));
                    entry.insert(Worker {
                        killed: Arc::new(AtomicBool::new(false)),
                        housekeeping: matches!(role.as_str(), "monitor" | "narrator"),
                        seen_at: Instant::now(),
                        read: None,
                        ended: false,
                    })
                }
            };
            if killed {
                worker.killed.store(true, Ordering::SeqCst);
            } else if !worker.housekeeping {
                working |= worker.holds(id, member["wake_mode"].as_str().unwrap_or_default());
            }
        }
        if working != self.held {
            self.held = working;
            out.push(Parsed::Task(if working {
                hold_started()
            } else {
                TaskEvent::Ended {
                    task_id: HOLD_TASK.into(),
                    status: TaskStatus::Done,
                    summary: None,
                    output: None,
                }
            }));
        }
        out
    }
}

impl Worker {
    /// Whether it is at work: mid-turn by its transcript and not stuck. One a board post has
    /// rung (`wake_mode` `working`) is waiting until its run resumes, which it is about to.
    fn holds(&mut self, id: &str, wake_mode: &str) -> bool {
        let rung = wake_mode == "working";
        let meta =
            transcript_path(id).and_then(|path| Some((std::fs::metadata(&path).ok()?, path)));
        let Some((meta, path)) = meta else {
            // No transcript yet: the roster says, as agy falls back to it.
            return rung && self.seen_at.elapsed() < STUCK;
        };
        let changed = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let read = (meta.len(), changed);
        if self.read != Some(read) {
            self.read = Some(read);
            self.ended = tail(&path).is_some_and(|tail| turn_ended(&tail));
        }
        let stuck = changed.elapsed().is_ok_and(|quiet| quiet > STUCK);
        (!self.ended || rung) && !stuck
    }
}

/// The call that stands for worker `name` (conversation `id`), finished at once as agy's call
/// that added it was; its chat then follows the worker's transcript.
fn added(id: &str, name: &str, role: &str, member: &Value, message_id: &str) -> [Parsed; 2] {
    let tool_id = format!("{SWARM_TASK_PREFIX}{id}");
    let title = member_title(name, role);
    let subagent = SubAgent {
        kind: if role.is_empty() { "worker" } else { role }.into(),
        description: title.clone(),
        model: member["model"]
            .as_str()
            .filter(|m| !m.is_empty())
            .map(String::from),
        prompt: None,
    };
    [
        tool_started(
            message_id,
            ToolStart {
                id: &tool_id,
                name: "Agent",
                summary: title,
                subagent: Some(subagent),
                parent: None,
            },
        ),
        tool_finished(message_id, &tool_id, false, ""),
    ]
}

fn hold_started() -> TaskEvent {
    TaskEvent::Started {
        task: BackgroundTask {
            tool_id: HOLD_TASK.into(),
            task_id: Some(HOLD_TASK.into()),
            run_id: None,
            kind: "agent".into(),
            description: "Swarm workers".into(),
            status: TaskStatus::Running,
            started_at: now_ms(),
            summary: None,
            output: None,
            cut_off: false,
        },
        task_id: HOLD_TASK.into(),
        top_level: true,
        backgrounded: true,
    }
}

/// A member's role, or the one its name gives a monitor or narrator on older rosters.
fn role_of(name: &str, member: &Value) -> String {
    let role = member["role"].as_str().map(str::trim).unwrap_or_default();
    if !role.is_empty() && role != "worker" {
        return role.to_ascii_lowercase();
    }
    let base = name.rsplit_once('-').map_or(name, |(base, n)| {
        if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() {
            base
        } else {
            name
        }
    });
    match base {
        "monitor" | "narrator" => base.into(),
        _ => role.into(),
    }
}

fn member_title(name: &str, role: &str) -> String {
    let handle = format!("@{}", one_line(name.trim_start_matches('@')));
    if role.is_empty() || role == "worker" {
        handle
    } else {
        format!("{handle} ({role})")
    }
}

/// The swarm roster of coordinator `session_id`.
fn read_roster(session_id: &str) -> Option<Value> {
    let path = conversation_dir(session_id)?.join(".system_generated/swarm/members.json");
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_MEMBERS_FILE_BYTES {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()
}

/// The complete lines at the end of a transcript, up to `MAX_TAIL_BYTES`, plus a torn last line.
fn tail(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(MAX_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_TAIL_BYTES).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // A read from the middle starts partway through a line.
    let text = if start > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    Some(text.to_owned())
}

/// Whether a transcript's newest step ends a turn, as agy's swarm board decides it: an answer
/// that called nothing, or the results of calls that included `swarm_wait` once its own result
/// is `DONE`. Anything newer, such as a system message, user input, a model error being retried,
/// or a torn last line, means the worker is mid-turn.
fn turn_ended(tail: &str) -> bool {
    // Whether each result after the newest answer is `DONE`, newest first.
    let mut results = Vec::new();
    for line in tail.lines().rev().filter(|line| !line.trim().is_empty()) {
        let Ok(step) = serde_json::from_str::<Value>(line) else {
            return false;
        };
        match step["type"].as_str() {
            Some("GENERIC") => {
                results.push(step["status"] == "DONE");
                continue;
            }
            Some("PLANNER_RESPONSE") => {}
            _ => return false,
        }
        let calls = step["tool_calls"].as_array().map_or(&[][..], Vec::as_slice);
        if calls.is_empty() {
            return results.is_empty();
        }
        results.reverse();
        let wait = calls.iter().position(|call| call["name"] == "swarm_wait");
        return wait.is_some_and(|wait| results.len() >= calls.len() && results[wait]);
    }
    false
}

/// Formats `swarm`, `post_message`, `read_board`, and `swarm_wait` tool calls cleanly.
pub(super) fn format_swarm_tool(name: &str, args: &Value) -> Option<(&'static str, String)> {
    let clean_handle = |key: &str| {
        args[key]
            .as_str()
            .map(|s| s.trim().trim_start_matches('@'))
            .filter(|s| !s.is_empty())
    };
    let clean_channel = |key: &str| {
        args[key]
            .as_str()
            .map(|s| s.trim().trim_start_matches('#'))
            .filter(|s| !s.is_empty())
    };

    match name {
        "swarm" => {
            let action = args["action"]
                .as_str()
                .map(str::trim)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let handle = clean_handle("name");
            let summary = match action.as_str() {
                "add" => {
                    let tasks = args["tasks"].as_array().map_or(0, Vec::len);
                    let n = args["n"]
                        .as_u64()
                        .and_then(|v| usize::try_from(v).ok())
                        .unwrap_or(0);
                    let count = tasks.max(n).min(MAX_SWARM_ADD_LAUNCHES);
                    match (count > 1, handle) {
                        (true, Some(h)) => format!("Add {count} swarm workers (@{h}-1..{count})"),
                        (true, None) => format!("Add {count} swarm workers"),
                        (false, Some(h)) => format!("Add swarm worker @{h}"),
                        (false, None) => "Add swarm worker".into(),
                    }
                }
                "kill" => match handle {
                    Some(h) => format!("Stop swarm worker @{h}"),
                    None => "Stop swarm worker".into(),
                },
                "status" => "Check swarm status".into(),
                "models" => "List swarm models".into(),
                "model" => {
                    let model = args["model"].as_str().map(str::trim).unwrap_or_default();
                    match (handle, !model.is_empty()) {
                        (Some(h), true) => format!("Switch @{h} model to {model}"),
                        (Some(h), false) => format!("Switch swarm model for @{h}"),
                        (None, true) => format!("Switch swarm model to {model}"),
                        (None, false) => "Switch swarm model".into(),
                    }
                }
                "title" => match args["title"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    Some(title) => format!("Set swarm title \"{}\"", one_line(title)),
                    None => "Set swarm title".into(),
                },
                "channel" => {
                    let op = args["op"].as_str().map(str::trim).unwrap_or_default();
                    match (op.is_empty(), clean_channel("channel")) {
                        (false, Some(ch)) => format!("Swarm channel {op} #{ch}"),
                        (false, None) => format!("Swarm channel {op}"),
                        (true, _) => "Manage swarm channels".into(),
                    }
                }
                "join" => match args["swarm"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    Some(s) => format!("Join swarm {s}"),
                    None => "Join swarm".into(),
                },
                _ => "Run swarm".into(),
            };
            Some(("Swarm", summary))
        }
        "post_message" => {
            let summary = match clean_channel("channel") {
                Some(ch) if ch != "main" => format!("Post to #{ch}"),
                _ => "Post to swarm board".into(),
            };
            Some(("Swarm", summary))
        }
        "read_board" => {
            let summary = if args["status"].as_bool() == Some(true) {
                "Read swarm status".into()
            } else if let Some(ch) = clean_channel("channel")
                && ch != "main"
            {
                format!("Read #{ch}")
            } else {
                "Read swarm board".into()
            };
            Some(("Swarm", summary))
        }
        "swarm_wait" => Some(("Swarm", "Wait for swarm".into())),
        _ => None,
    }
}

/// Builds a `SubAgent` descriptor for classic `invoke_subagent` calls when present.
pub(super) fn invoke_subagent_descriptor(args: &Value) -> Option<SubAgent> {
    if let Some(role) = args["Role"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let prompt = args["Prompt"].as_str().map(String::from);
        let model = args["Model"]
            .as_str()
            .filter(|m| !m.is_empty() && *m != "inherit")
            .map(String::from);
        return Some(SubAgent {
            kind: args["TypeName"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("subagent")
                .into(),
            description: role.into(),
            model,
            prompt,
        });
    }
    let first = args["Subagents"].as_array()?.first()?;
    let role = first["Role"].as_str()?.trim();
    if role.is_empty() {
        return None;
    }
    Some(SubAgent {
        kind: "subagent".into(),
        description: role.into(),
        model: None,
        prompt: first["Prompt"].as_str().map(String::from),
    })
}
