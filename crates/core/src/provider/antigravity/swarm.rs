//! Native Antigravity ACP swarm tracking (`_antigravity/swarm`), disk fallback (`members.json`),
//! worker subagent routing, and clean swarm tool presentation.

// Rust guideline compliant 2026-11-22

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::chats::{BackgroundTask, SubAgent, TaskStatus};
use crate::events::AgentEvent;
use crate::provider::{Parsed, ToolStart, TurnSpec, one_line, tool_finished, tool_started};
use crate::system::now_ms;

use super::client::home;
use super::parse::parse_session_update;

/// Prefix for synthetic tool/task ids representing swarm workers (`swarm:<conversationId>`).
pub const SWARM_TASK_PREFIX: &str = "swarm:";

/// Synthetic task id holding stdin open while the coordinator waits on a board wake.
const COORDINATOR_WAKE_TASK: &str = "swarm:coordinator";

/// Housekeeping roles whose periodic pulses must not hold a swarm turn open.
const HOUSEKEEPING_ROLES: &[&str] = &["monitor", "narrator"];

/// Upper bound on batch swarm worker launches parsed from untrusted tool input.
const MAX_SWARM_ADD_LAUNCHES: usize = 64;

/// Maximum board report characters attached to a worker subagent result.
const MAX_BOARD_REPORT_CHARS: usize = 8_000;

/// Maximum `members.json` file size read during the disk fallback (4 MiB).
const MAX_MEMBERS_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Capabilities metadata advertising native Antigravity swarm notification support.
pub(super) fn capabilities_meta() -> Value {
    json!({"antigravity": {"swarm": true}})
}

/// Child worker metadata attached to an ACP notification or permission request.
#[derive(Debug, Clone)]
pub struct ChildTarget {
    pub child_session_id: String,
    pub worker_label: Option<String>,
}

/// Extracts child worker routing metadata from an ACP notification or permission request.
#[must_use]
pub fn extract_child_target(v: &Value, own_session_id: Option<&str>) -> Option<ChildTarget> {
    let params = &v["params"];
    let candidates = [
        &params["_meta"],
        &params["update"]["_meta"],
        &params["toolCall"]["_meta"],
    ];
    for candidate in candidates {
        let ag = &candidate["antigravity"];
        if !ag.is_object() {
            continue;
        }
        let swarm = &ag["swarm"];
        let child_id = ag["childSessionId"]
            .as_str()
            .or_else(|| swarm["workerSessionId"].as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(child_id) = child_id {
            let worker_name = swarm["workerName"]
                .as_str()
                .or_else(|| ag["agentName"].as_str())
                .map(|s| s.trim().trim_start_matches('@'))
                .filter(|s| !s.is_empty());
            let role = ag["role"].as_str().map(str::trim).filter(|s| !s.is_empty());
            let worker_label = worker_name
                .map(|n| format!("@{n}"))
                .or_else(|| role.map(String::from));
            return Some(ChildTarget {
                child_session_id: child_id.to_string(),
                worker_label,
            });
        }
    }
    // Fallback: a `session/update` whose `sessionId` differs from the root conversation id.
    if let (Some(own), Some(sid)) = (own_session_id, params["sessionId"].as_str())
        && !sid.is_empty()
        && sid != own
    {
        return Some(ChildTarget {
            child_session_id: sid.to_string(),
            worker_label: None,
        });
    }
    None
}

/// Maps a child worker's ACP `session/update` notification into events for its child chat.
#[must_use]
pub fn subagent_events(v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<AgentEvent> {
    if v["method"].as_str() != Some("session/update") {
        return Vec::new();
    }
    let mut parsed = Vec::new();
    parse_session_update(&v["params"]["update"], spec, message_id, &mut parsed);
    parsed
        .into_iter()
        .filter_map(|p| match p {
            Parsed::Event(event) => Some(event),
            _ => None,
        })
        .collect()
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

/// Lifecycle of a single tracked swarm worker inside one turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerPhase {
    Running,
    Parked,
    Finished,
}

#[derive(Debug, Clone)]
struct TrackedMember {
    name: String,
    phase: WorkerPhase,
    saw_idle_or_report: bool,
    latest_report: Option<String>,
}

/// Per-turn state tracking Antigravity native `_antigravity/swarm` notifications and
/// `swarm/members.json` fallback so swarm turns stay active until all workers report.
#[derive(Debug, Default)]
pub struct SwarmState {
    native_protocol: bool,
    saw_swarm_tool: bool,
    coordinator_held: bool,
    members: HashMap<String, TrackedMember>,
    finished_ids: HashSet<String>,
    latest_posts: HashMap<String, String>,
}

impl SwarmState {
    /// Marks that this turn called a `swarm` tool, enabling the disk fallback on older builds.
    pub fn note_swarm_tool(&mut self) {
        self.saw_swarm_tool = true;
    }

    /// Whether any non-housekeeping swarm worker or coordinator wake is currently active.
    #[must_use]
    pub fn has_live_work(&self) -> bool {
        self.coordinator_held
            || self
                .members
                .values()
                .any(|m| m.phase == WorkerPhase::Running)
    }

    /// Applies one `_antigravity/swarm` JSON-RPC notification and returns Patchwork events and
    /// background task updates.
    pub fn apply_notification(&mut self, params: &Value, message_id: &str) -> Vec<Parsed> {
        self.native_protocol = true;
        let mut out = Vec::new();

        // Capture latest board posts per worker handle.
        for post in params["posts"].as_array().into_iter().flatten() {
            let Some(agent) = post["agent"]
                .as_str()
                .map(|s| s.trim().trim_start_matches('@').to_ascii_lowercase())
                .filter(|s| !s.is_empty() && s != "coordinator")
            else {
                continue;
            };
            if let Some(text) = post["text"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                self.latest_posts
                    .insert(agent, clip_chars(text, MAX_BOARD_REPORT_CHARS));
            }
        }

        let mut live_task_ids: Vec<String> = Vec::new();
        let mut any_non_housekeeping_running = false;

        for member in params["members"].as_array().into_iter().flatten() {
            let Some(conv_id) = member["conversationId"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let role = member["role"]
                .as_str()
                .map_or("worker", str::trim)
                .to_ascii_lowercase();
            if role == "coordinator" {
                continue;
            }
            let housekeeping = HOUSEKEEPING_ROLES.contains(&role.as_str());
            let name = member["name"]
                .as_str()
                .map(|s| s.trim().trim_start_matches('@'))
                .filter(|s| !s.is_empty())
                .unwrap_or(conv_id);
            let name_key = name.to_ascii_lowercase();
            let state = member["state"]
                .as_str()
                .map(str::trim)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let tool_id = format!("{SWARM_TASK_PREFIX}{conv_id}");
            let title = format_member_title(name, &role, member["addedBy"].as_str());
            let report = self.latest_posts.get(&name_key).cloned();

            // Do not recreate an already-finished worker from replayed rosters.
            if self.finished_ids.contains(conv_id) {
                continue;
            }

            let is_new = !self.members.contains_key(conv_id);
            if is_new {
                // A recreated worker with a reused name must not inherit an old report.
                if self
                    .members
                    .values()
                    .any(|m| m.name.eq_ignore_ascii_case(name) && m.phase == WorkerPhase::Finished)
                {
                    self.latest_posts.remove(&name_key);
                }
                let subagent = SubAgent {
                    kind: if role.is_empty() {
                        "worker".into()
                    } else {
                        role.clone()
                    },
                    description: title.clone(),
                    model: member["modelLabel"]
                        .as_str()
                        .or_else(|| member["model"].as_str())
                        .map(String::from),
                    prompt: member["brief"].as_str().map(String::from),
                };
                out.push(tool_started(
                    message_id,
                    ToolStart {
                        id: &tool_id,
                        name: "Agent",
                        summary: title.clone(),
                        subagent: Some(subagent),
                        parent: None,
                    },
                ));
                self.members.insert(
                    conv_id.to_string(),
                    TrackedMember {
                        name: name.to_string(),
                        phase: WorkerPhase::Parked,
                        saw_idle_or_report: false,
                        latest_report: None,
                    },
                );
            }

            let Some(tracked) = self.members.get_mut(conv_id) else {
                continue;
            };
            if let Some(ref text) = report {
                tracked.saw_idle_or_report = true;
                tracked.latest_report = Some(text.clone());
            }

            match state.as_str() {
                "working" => {
                    if !housekeeping {
                        any_non_housekeeping_running = true;
                        live_task_ids.push(tool_id.clone());
                    }
                    if tracked.phase != WorkerPhase::Running {
                        tracked.phase = WorkerPhase::Running;
                        if !housekeeping {
                            out.push(Parsed::Task(crate::provider::TaskEvent::Started {
                                task: BackgroundTask {
                                    tool_id: tool_id.clone(),
                                    task_id: Some(tool_id.clone()),
                                    run_id: None,
                                    kind: "agent".into(),
                                    description: title,
                                    status: TaskStatus::Running,
                                    started_at: now_ms(),
                                    summary: None,
                                    output: None,
                                    cut_off: false,
                                },
                                task_id: tool_id,
                                top_level: true,
                                backgrounded: true,
                            }));
                        }
                    }
                }
                "waiting" | "stopped" => {
                    tracked.saw_idle_or_report = true;
                    if tracked.phase == WorkerPhase::Running {
                        tracked.phase = WorkerPhase::Parked;
                        if !housekeeping {
                            out.push(Parsed::Task(crate::provider::TaskEvent::Ended {
                                task_id: tool_id,
                                status: TaskStatus::Done,
                                summary: tracked.latest_report.as_deref().map(one_line),
                                output: tracked.latest_report.clone(),
                            }));
                        }
                    }
                }
                "failed" => {
                    tracked.phase = WorkerPhase::Finished;
                    self.finished_ids.insert(conv_id.to_string());
                    let failure = member["failure"]
                        .as_str()
                        .map(String::from)
                        .or_else(|| tracked.latest_report.clone())
                        .unwrap_or_else(|| "Swarm worker failed".into());
                    out.push(Parsed::Task(crate::provider::TaskEvent::Ended {
                        task_id: tool_id.clone(),
                        status: TaskStatus::Failed,
                        summary: Some(one_line(&failure)),
                        output: Some(failure.clone()),
                    }));
                    out.push(tool_finished(message_id, &tool_id, true, &failure));
                }
                "killed" => {
                    let completed =
                        tracked.saw_idle_or_report || member["hadActivity"].as_bool() == Some(true);
                    tracked.phase = WorkerPhase::Finished;
                    self.finished_ids.insert(conv_id.to_string());
                    let output = tracked.latest_report.clone().unwrap_or_default();
                    out.push(Parsed::Task(crate::provider::TaskEvent::Ended {
                        task_id: tool_id.clone(),
                        status: if completed {
                            TaskStatus::Done
                        } else {
                            TaskStatus::Stopped
                        },
                        summary: (!output.is_empty()).then(|| one_line(&output)),
                        output: (!output.is_empty()).then_some(output.clone()),
                    }));
                    out.push(tool_finished(message_id, &tool_id, !completed, &output));
                }
                _ => {}
            }
        }

        let coordinator_busy = params["coordinatorPendingWake"].as_bool() == Some(true)
            || params["coordinatorWorking"].as_bool() == Some(true);
        let hold = any_non_housekeeping_running || coordinator_busy;
        if hold {
            live_task_ids.push(COORDINATOR_WAKE_TASK.to_string());
            if !self.coordinator_held {
                self.coordinator_held = true;
                out.push(Parsed::Task(crate::provider::TaskEvent::Started {
                    task: BackgroundTask {
                        tool_id: COORDINATOR_WAKE_TASK.into(),
                        task_id: Some(COORDINATOR_WAKE_TASK.into()),
                        run_id: None,
                        kind: "agent".into(),
                        description: "Swarm coordinator".into(),
                        status: TaskStatus::Running,
                        started_at: now_ms(),
                        summary: None,
                        output: None,
                        cut_off: false,
                    },
                    task_id: COORDINATOR_WAKE_TASK.into(),
                    top_level: true,
                    backgrounded: false,
                }));
            }
        } else if self.coordinator_held {
            self.coordinator_held = false;
            out.push(Parsed::Task(crate::provider::TaskEvent::Ended {
                task_id: COORDINATOR_WAKE_TASK.into(),
                status: TaskStatus::Done,
                summary: None,
                output: None,
            }));
        }

        out.push(Parsed::Task(crate::provider::TaskEvent::Level {
            live: live_task_ids,
        }));
        out
    }

    /// Disk fallback for released `agy` 1.5.x builds that do not emit `_antigravity/swarm`:
    /// reads `swarm/members.json` under `~/.gemini` to keep active swarm workers alive.
    pub fn poll_disk_fallback(&mut self, session_id: &str, message_id: &str) -> Vec<Parsed> {
        if self.native_protocol || !self.saw_swarm_tool || session_id.is_empty() {
            return Vec::new();
        }
        let Some(raw) = read_members_file(session_id) else {
            return Vec::new();
        };
        let Ok(roster) = serde_json::from_str::<Value>(&raw) else {
            return Vec::new();
        };
        let Some(members) = roster["members"].as_object() else {
            return Vec::new();
        };

        let mut out = Vec::new();
        let mut live_task_ids = Vec::new();
        for (name, member) in members {
            let Some(conv_id) = member["conversation_id"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let role = member["role"]
                .as_str()
                .unwrap_or("worker")
                .to_ascii_lowercase();
            if role == "coordinator" || HOUSEKEEPING_ROLES.contains(&role.as_str()) {
                continue;
            }
            let killed = member["desired_state"].as_str() == Some("KILLED")
                || member["phase"].as_str() == Some("KILLED");
            let parked = matches!(member["wake_mode"].as_str(), Some("mentions" | "anything"));
            let tool_id = format!("{SWARM_TASK_PREFIX}{conv_id}");
            let title = format_member_title(name, &role, None);

            if !self.members.contains_key(conv_id) && !self.finished_ids.contains(conv_id) {
                out.push(tool_started(
                    message_id,
                    ToolStart {
                        id: &tool_id,
                        name: "Agent",
                        summary: title.clone(),
                        subagent: Some(SubAgent {
                            kind: role.clone(),
                            description: title.clone(),
                            model: None,
                            prompt: None,
                        }),
                        parent: None,
                    },
                ));
                self.members.insert(
                    conv_id.to_string(),
                    TrackedMember {
                        name: name.clone(),
                        phase: WorkerPhase::Parked,
                        saw_idle_or_report: false,
                        latest_report: None,
                    },
                );
            }
            let Some(tracked) = self.members.get_mut(conv_id) else {
                continue;
            };
            if killed {
                if tracked.phase != WorkerPhase::Finished {
                    tracked.phase = WorkerPhase::Finished;
                    self.finished_ids.insert(conv_id.to_string());
                    out.push(Parsed::Task(crate::provider::TaskEvent::Ended {
                        task_id: tool_id.clone(),
                        status: TaskStatus::Done,
                        summary: None,
                        output: None,
                    }));
                    out.push(tool_finished(message_id, &tool_id, false, ""));
                }
            } else if !parked {
                live_task_ids.push(tool_id.clone());
                if tracked.phase != WorkerPhase::Running {
                    tracked.phase = WorkerPhase::Running;
                    out.push(Parsed::Task(crate::provider::TaskEvent::Started {
                        task: BackgroundTask {
                            tool_id: tool_id.clone(),
                            task_id: Some(tool_id.clone()),
                            run_id: None,
                            kind: "agent".into(),
                            description: title,
                            status: TaskStatus::Running,
                            started_at: now_ms(),
                            summary: None,
                            output: None,
                            cut_off: false,
                        },
                        task_id: tool_id,
                        top_level: true,
                        backgrounded: true,
                    }));
                }
            }
        }
        out.push(Parsed::Task(crate::provider::TaskEvent::Level {
            live: live_task_ids,
        }));
        out
    }
}

fn format_member_title(name: &str, role: &str, added_by: Option<&str>) -> String {
    let handle = format!("@{}", name.trim_start_matches('@'));
    let role_suffix = if !role.is_empty() && role != "worker" {
        format!(" ({role})")
    } else {
        String::new()
    };
    let parent_suffix = added_by
        .map(|s| s.trim().trim_start_matches('@'))
        .filter(|s| !s.is_empty() && *s != "coordinator")
        .map_or_else(String::new, |by| format!(" via @{by}"));
    format!("{handle}{role_suffix}{parent_suffix}")
}

fn read_members_file(session_id: &str) -> Option<String> {
    if session_id.contains(['/', '\\', '\0']) || session_id == "." || session_id == ".." {
        return None;
    }
    let root = home()?;
    for app_dir in ["antigravity-cli", "antigravity-acp"] {
        let path: PathBuf = root
            .join(app_dir)
            .join("brain")
            .join(session_id)
            .join(".system_generated")
            .join("swarm")
            .join("members.json");
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.is_file() && meta.len() <= MAX_MEMBERS_FILE_BYTES {
            return std::fs::read_to_string(&path).ok();
        }
    }
    None
}

fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        text.chars().take(max).collect()
    }
}
