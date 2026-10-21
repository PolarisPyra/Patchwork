//! Shared data contract between the Rust backend and the TypeScript frontend.
//! Mirrors `apps/web/src/api.ts` — keep both in sync. All fields serialize as camelCase.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
    /// Saved as "gemini" before the rename; the alias keeps those threads and settings readable.
    #[serde(alias = "gemini")]
    Antigravity,
}

impl AgentKind {
    pub const ALL: [AgentKind; 3] = [AgentKind::Claude, AgentKind::Codex, AgentKind::Antigravity];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    #[default]
    Dark,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Integrations {
    pub github: bool,
    pub terminal: bool,
    pub figma: bool,
    pub browser: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub onboarded: bool,
    pub theme: Theme,
    /// Connected agents; the first is the default for new tasks.
    pub agents: Vec<AgentKind>,
    /// Legacy single-agent setting, migrated into `agents` on load.
    #[serde(skip_serializing)]
    pub agent: Option<AgentKind>,
    pub integrations: Integrations,
    pub projects: Vec<Project>,
    pub active_project_id: Option<String>,
    pub tokens: TokenSettings,
    /// Shell that agents run commands in. None follows the SHELL environment variable.
    pub shell: Option<String>,
    pub notifications: NotificationSettings,
}

/// Desktop notifications shown while Patchwork is not the focused window.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationSettings {
    /// When a task's goal is met.
    pub goal_complete: bool,
    /// When an agent finishes replying in a task that is not pursuing a goal.
    pub task_complete: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            goal_complete: true,
            task_complete: true,
        }
    }
}

/// Settings that trade a little convenience for fewer tokens spent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TokenSettings {
    /// Shows the token counts under each finished reply.
    pub show_usage: bool,
    /// A context above this is flagged, and a cold task this large is asked about first.
    pub warn_above: u64,
    /// Asks before a large task whose prompt cache has expired is resumed.
    pub ask_before_resume: bool,
    /// Compacts an idle Claude task just before its prompt cache expires.
    pub auto_compact: bool,
    /// The context size above which an idle task is compacted.
    pub auto_compact_above: u64,
    /// Model for new tasks, per agent. A missing agent uses the CLI's own default.
    pub default_models: HashMap<AgentKind, String>,
}

impl Default for TokenSettings {
    fn default() -> Self {
        Self {
            show_usage: true,
            warn_above: 150_000,
            ask_before_resume: true,
            auto_compact: true,
            auto_compact_above: 200_000,
            default_models: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub kind: AgentKind,
    pub installed: bool,
    pub version: Option<String>,
    pub signed_in: bool,
    /// Human-readable one-liner, e.g. "Signed in with claude.ai" or "Not installed".
    pub detail: String,
}

/// A model the user can pick for an agent. `id: None` is the CLI's own default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: Option<String>,
    pub label: String,
    /// Configured CLI default; not a claim about a resumed session's runtime model.
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Streaming,
    Complete,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
}

/// A shell command the agent started in the background, still running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundTask {
    /// The id of the tool call that started it; stops it.
    pub tool_id: String,
    /// What the command does, as the agent described it.
    pub description: String,
}

/// A subagent an agent started, e.g. with Claude Code's Task tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubAgent {
    /// The subagent type, e.g. "general-purpose" or "Explore".
    pub kind: String,
    /// What the subagent was asked to do.
    pub description: String,
    /// Model the agent chose for it; None means it inherits the parent's.
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub text: String,
    pub status: StepStatus,
}

/// How much an agent may do without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AccessMode {
    /// Ask before commands and file changes.
    Supervised,
    /// Edits are approved; other actions ask.
    AutoAcceptEdits,
    /// The provider approves routine actions; others still ask.
    Auto,
    #[default]
    FullAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalStatus {
    Pending,
    Allowed,
    Denied,
}

/// An action the agent asked permission for during a turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: String,
    /// e.g. "Run command", "Edit files", "Use WebFetch".
    pub title: String,
    /// The command, paths, or other detail being approved.
    pub detail: String,
    pub status: ApprovalStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Image,
    File,
    Folder,
}

/// Something dropped into the composer, sent to the agent with the message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Tokens a reply used, summed over the model requests it made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    /// Input that was neither read from nor written to the prompt cache.
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
    /// Size of the prompt of the agent's last request: the context it is working in.
    pub context: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
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
    /// The agent's reasoning while it works. Live only: it is not stored.
    #[serde(default)]
    pub thinking: String,
    /// When the reply finished (ms since epoch); None while it runs.
    #[serde(default)]
    pub finished_at: Option<u64>,
    /// Tokens the reply has used; None for agents that do not report usage.
    #[serde(default)]
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    /// None for a chat without a project; it runs in a private scratch folder.
    pub project_id: Option<String>,
    pub title: String,
    pub agent: AgentKind,
    pub model: Option<String>,
    #[serde(default)]
    pub access_mode: AccessMode,
    /// Reasoning effort passed to the agent, e.g. "high"; None leaves the CLI's default.
    #[serde(default)]
    pub effort: Option<String>,
    /// Claude session_id / Codex thread_id used to resume the conversation.
    pub session_id: Option<String>,
    pub created_at: u64,
    pub messages: Vec<Message>,
    /// The `/goal` the agent is working toward, as the agent last reported it.
    #[serde(default)]
    pub goal: Option<Goal>,
    /// Sidebar order, lower first. Kept by the store; the UI gets threads already sorted.
    #[serde(skip)]
    pub position: i64,
    /// The folder this task is in; None for the project's top level.
    #[serde(default)]
    pub folder_id: Option<String>,
    /// When the thread was archived; None while it is in the task list.
    #[serde(default)]
    pub archived_at: Option<u64>,
}

/// A folder inside a project. Its threads are those whose `folder_id` names it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub project_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

/// A condition the agent keeps working toward until it is met (`/goal`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub objective: String,
    pub status: GoalStatus,
    /// Unix epoch milliseconds.
    pub started_at: u64,
    /// Evaluator checks that came back unmet (Claude).
    pub checks: Option<u32>,
    /// What the agent reports having spent on the goal (Codex).
    pub tokens_used: Option<u64>,
    pub token_budget: Option<u64>,
    pub time_used_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    /// "modified" | "added" | "deleted" | "untracked" | "renamed"
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub staged: bool,
}

/// One rate-limit window, e.g. the 5-hour session limit.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub label: String,
    /// 0–100.
    pub used_percent: f64,
    /// Unix epoch milliseconds.
    pub resets_at: Option<u64>,
}

/// Subscription usage limits for one agent, as reported by its provider.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsage {
    pub kind: AgentKind,
    /// e.g. "max", "plus".
    pub plan: Option<String>,
    pub windows: Vec<UsageWindow>,
    /// Why no windows are shown, e.g. "Not signed in".
    pub error: Option<String>,
    /// When the provider last reported these numbers (epoch ms).
    pub updated_at: Option<u64>,
}

/// Where imported conversations come from: an agent CLI's own session
/// files, or another app (T3 Code) that ran those agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportSource {
    Claude,
    Codex,
    #[serde(alias = "gemini")]
    Antigravity,
    T3,
}

/// Conversations a source has saved on disk that can be imported.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportScan {
    pub kind: ImportSource,
    /// Sessions found on disk.
    pub found: u32,
    /// Sessions not yet in Patchwork.
    pub new: u32,
    /// Helper runs inside other conversations; not offered for import.
    pub subagents: u32,
}

/// Emitted on the Tauri event channel `"agent-event"`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEventEnvelope {
    pub thread_id: String,
    pub event: AgentEvent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentEvent {
    /// A new assistant message placeholder (status = streaming).
    MessageStarted {
        message: Message,
    },
    /// Append `text` to the message body.
    TextDelta {
        message_id: String,
        text: String,
    },
    /// Part of the agent's reasoning, shown while it works and not stored.
    Thinking {
        message_id: String,
        text: String,
    },
    ToolStarted {
        message_id: String,
        tool: ToolCall,
    },
    ToolFinished {
        message_id: String,
        tool_id: String,
        is_error: bool,
    },
    /// Replace the message's plan with `steps`.
    PlanUpdated {
        message_id: String,
        steps: Vec<PlanStep>,
    },
    FileTouched {
        message_id: String,
        path: String,
    },
    /// The agent is waiting for the user to allow or deny `approval`.
    ApprovalRequested {
        message_id: String,
        approval: Approval,
    },
    ApprovalResolved {
        message_id: String,
        approval_id: String,
        allowed: bool,
    },
    /// The thread's goal changed; None once it is cleared.
    GoalUpdated {
        goal: Option<Goal>,
    },
    /// The tokens a reply has used so far; replaces the count it had.
    Usage {
        message_id: String,
        usage: TokenUsage,
    },
    /// A message sent mid-turn is now waiting for the turn to end (`queued`), or no longer is.
    MessageQueued {
        message_id: String,
        queued: bool,
    },
    /// A queued message was taken back, so it leaves the thread.
    MessageRemoved {
        message_id: String,
    },
    /// A background shell command started and is still running.
    BackgroundTaskStarted {
        message_id: String,
        task: BackgroundTask,
    },
    /// A background shell command ended.
    BackgroundTaskEnded {
        message_id: String,
        tool_id: String,
    },
    /// Terminal event for a turn. `status` is complete | error | cancelled.
    TurnCompleted {
        message_id: String,
        status: MessageStatus,
        error: Option<String>,
    },
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
