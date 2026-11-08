//! A message and what it carries: its parts, tool calls, approvals, attachments and usage.

use std::collections::{HashMap};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::permissions::{PermissionKind, RuleRef};

/// Which side of a chat a message came from.
///
/// `Assistant` is the role's name in the provider APIs, not a person: it is whichever agent
/// answered — Claude, Codex, OpenCode or the API — and that is the thread's `agent`, not the
/// message's. The values are sent to the providers as they are, so they are not ours to rename.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// What the person wrote.
    User,
    /// The agent's reply.
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Streaming,
    Complete,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    /// e.g. "Bash", "Edit", "command_execution"
    pub name: String,
    /// Short single-line description, e.g. the command or file path.
    pub summary: String,
    pub status: StepStatus,
    /// Set when the call started a subagent.
    #[serde(default)]
    pub subagent: Option<SubAgent>,
    /// The subagent call (its `id`) this call ran inside, if any.
    #[serde(default)]
    pub parent: Option<String>,
    /// When the call started and ended, in Unix epoch milliseconds.
    #[serde(default)]
    pub started_at: Option<u64>,
    #[serde(default)]
    pub ended_at: Option<u64>,
    /// What the call returned, cut to its first `MAX_TOOL_OUTPUT` bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// The thread holding the subagent's own transcript, for a call that started one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_thread_id: Option<String>,
    /// The lines an edit adds and removes, for an agent whose call carries the edit's text. For a
    /// `Write`, every line of its text, which is right only when it created the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<EditLines>,
}

/// How many lines an edit adds and removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
pub struct EditLines {
    pub added: u32,
    pub removed: u32,
}

/// The most of a tool call's output that is kept.
pub const MAX_TOOL_OUTPUT: usize = 16 << 10;

/// `output` cut to `MAX_TOOL_OUTPUT` bytes at a character boundary; None when empty.
pub fn tool_output(output: &str) -> Option<String> {
    let output = output.trim_end();
    if output.is_empty() {
        return None;
    }
    let mut end = output.len().min(MAX_TOOL_OUTPUT);
    while !output.is_char_boundary(end) {
        end -= 1;
    }
    Some(output[..end].to_string())
}

/// One step of a reply, in the order it happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Part {
    Text {
        text: String,
    },
    /// The agent's reasoning; `ended_at` is None while it still streams.
    Thinking {
        text: String,
        started_at: u64,
        ended_at: Option<u64>,
    },
    /// A tool call, by its id in the message's `tools`.
    Tool {
        id: String,
    },
    /// A plan the agent proposed in plan mode, as Markdown.
    Plan {
        text: String,
    },
    /// An approval or question, by its id in the message's `approvals`, where it was asked.
    Approval {
        id: String,
    },
    /// Where the agent compacted its context on its own, partway through the reply, with the
    /// summary it went on from when the agent reports one.
    Compaction {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
}

/// Where a task the agent started stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Running,
    /// Pending, or paused by the agent.
    Waiting,
    Done,
    Failed,
    /// Stopped by the user, ended with the process that ran it, or dropped from the tasks the
    /// agent still reports running.
    #[default]
    Stopped,
}

impl TaskStatus {
    /// Whether the task still runs, so it is listed under the background.
    pub fn is_live(self) -> bool {
        matches!(self, Self::Running | Self::Waiting)
    }
}

/// A task the agent started and left running past its reply, such as a background command, a
/// subagent, or a workflow. It is listed under the message it started in until it ends.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase", default)]
pub struct BackgroundTask {
    /// The id of the tool call that started it.
    pub tool_id: String,
    /// The id the agent gives the task. Stopping it takes this id, so None means it cannot be stopped.
    pub task_id: Option<String>,
    /// The agent's id for the run the task belongs to, when it has one.
    pub run_id: Option<String>,
    /// The kind of task: Claude's task type, such as `local_bash`, or `command` or `agent` for Codex.
    pub kind: String,
    /// What the task does, as the agent described it.
    pub description: String,
    pub status: TaskStatus,
    /// When it started, in Unix epoch milliseconds.
    pub started_at: u64,
    /// The latest one-line progress the agent reported.
    pub summary: Option<String>,
    /// The file its output was written to.
    pub output: Option<String>,
}

/// A subagent an agent started, e.g. with Claude Code's Task tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct SubAgent {
    /// The subagent type, e.g. "general-purpose" or "Explore".
    pub kind: String,
    /// What the subagent was asked to do.
    pub description: String,
    /// Model the agent chose for it; None means it inherits the parent's.
    pub model: Option<String>,
    /// The instructions it was given.
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub text: String,
    pub status: StepStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "lowercase")]
pub enum ApprovalStatus {
    Pending,
    Allowed,
    Denied,
}

/// An action the agent asked permission for during a turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: String,
    /// e.g. "Run command", "Edit files", "Use WebFetch".
    pub title: String,
    /// The command, paths, or other detail being approved.
    pub detail: String,
    pub status: ApprovalStatus,
    /// What the agent asks the user, when it asks rather than wanting permission.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<Question>,
    /// The user's answers, by question id.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub answers: HashMap<String, Vec<String>>,
    /// What kind of request it is, when a permission rule can answer it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<PermissionKind>,
    /// The patterns "Always allow" saves; empty when a rule could not safely cover it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
    /// The rule that answered it, or that the user saved when answering it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<RuleRef>,
    /// Answered by a rule, without asking.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto: bool,
    /// Set when the agent asks for a secret, which the user enters in a private card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<crate::secrets::Secret>,
}

/// A question the agent asks the user, with the answers it offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct Question {
    /// The key its answer goes back under.
    pub id: String,
    /// A short label, such as "Library".
    pub header: String,
    pub question: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Image,
    File,
    Folder,
}

/// Something dropped into the composer, sent to the agent with the message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub kind: AttachmentKind,
    /// Absolute path on disk.
    pub path: String,
    /// Last path component, for display.
    pub name: String,
}

/// Media type for the image formats the agents accept, by file extension.
pub fn image_media_type(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

impl Attachment {
    /// Classifies a path that exists on disk.
    pub fn from_path(path: &str) -> Result<Self, String> {
        let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
        let kind = if meta.is_dir() {
            AttachmentKind::Folder
        } else if image_media_type(path).is_some() {
            AttachmentKind::Image
        } else {
            AttachmentKind::File
        };
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        Ok(Self {
            kind,
            path: path.to_string(),
            name,
        })
    }
}

/// Why a safety classifier blocked a reply, as the agent reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    /// The policy area, such as `cyber`; None when the agent names none.
    pub category: Option<String>,
    pub explanation: String,
}

/// Tokens a reply used, summed over the model requests it made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    /// Input that was neither read from nor written to the prompt cache.
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
    /// Size of the prompt of the agent's last request: the context it is working in.
    pub context: u64,
    /// How long the last request took to its first token, in milliseconds. Only Patchwork's own
    /// agent measures it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub first_token_ms: Option<u64>,
    /// The last request's output tokens per second after its first token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub output_per_second: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    /// Who spoke: the person, or the agent that answered (see `Role`).
    pub role: Role,
    pub text: String,
    /// Unix epoch milliseconds.
    pub created_at: u64,
    pub status: MessageStatus,
    #[serde(default)]
    pub tools: Vec<ToolCall>,
    #[serde(default)]
    pub plan: Vec<PlanStep>,
    /// Project-relative paths the agent edited during this message.
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub approvals: Vec<Approval>,
    /// Files, folders, and images the user dropped into the message.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// Sent while a turn was running, to steer that turn rather than start one.
    #[serde(default)]
    pub steer: bool,
    /// Sent while a turn was running and still waiting for it to end; then it starts the next turn.
    #[serde(default)]
    pub queued: bool,
    /// Background shell commands still running after the reply ended; they keep the turn going.
    #[serde(default)]
    pub background: Vec<BackgroundTask>,
    /// Cut short by a steering message; the agent answered it in the next reply.
    #[serde(default)]
    pub cut_off: bool,
    /// A safety classifier blocked part of the reply; the agent's stated reason.
    #[serde(default)]
    pub refusal: Option<Refusal>,
    /// The reply's text, reasoning, and tool calls in the order they happened. Replies saved
    /// before this was kept have none.
    #[serde(default)]
    pub parts: Vec<Part>,
    /// When the reply finished (ms since epoch); None while it runs.
    #[serde(default)]
    pub finished_at: Option<u64>,
    /// Tokens the reply has used; None for agents that do not report usage.
    #[serde(default)]
    pub usage: Option<TokenUsage>,
    /// The project before and after the turn that wrote the reply, so its changes can be shown
    /// after they are committed or edited again. None outside git projects and for older replies.
    #[serde(default)]
    pub turn: Option<TurnTrees>,
    /// What the project's own check command made of the turn, for a reply whose turn changed
    /// files. None for older replies, and for projects with nothing to run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks: Option<TurnChecks>,
}

/// Snapshots of a project's files as git tree ids, kept alive by refs under `refs/patchwork`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct TurnTrees {
    pub before: String,
    pub after: String,
}

/// How a turn's check command stands. `NotRun` is not a pass: a reply whose checks were never
/// run says so, rather than looking like one that passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub enum CheckStatus {
    Running,
    Passed,
    Failed,
    /// It ran past its time limit, so what it would have said is unknown.
    TimedOut,
    /// Nothing ran; `TurnChecks::note` says why.
    NotRun,
}

/// What a project's check command made of the turn that wrote a reply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct TurnChecks {
    pub status: CheckStatus,
    /// The command that ran, as the person would type it in the project's folder. None when
    /// there was nothing to run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// How long it ran, in milliseconds. Zero while it runs, and when nothing ran.
    #[serde(default)]
    pub duration_ms: u64,
    /// How many tests the output said ran. None when it did not say, as a type check's output
    /// does not. `Some(0)` is a command that ran none at all, which is not a pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests: Option<u32>,
    /// The things that failed, each named on its own. Empty when it passed or didn't run.
    #[serde(default)]
    pub failures: Vec<CheckFailure>,
    /// The end of what it printed, for the reply's Checks card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Why nothing ran, or that it ran past its limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Whether this project runs its checks after every task, as it stood when this ran. Off, a
    /// run was asked for, and the card offers to turn them on.
    #[serde(default)]
    pub enabled: bool,
    /// When it started, in Unix epoch milliseconds.
    pub at: u64,
}

/// One thing the check command reported as failing: a test, a check, a build step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "threads.ts")]
#[serde(rename_all = "camelCase")]
pub struct CheckFailure {
    /// What failed, as it reads in the output ("tests::it_works", "FAIL src/app.test.ts").
    pub name: String,
    /// The line or two that says why, when the output gave one.
    #[serde(default)]
    pub detail: String,
}
