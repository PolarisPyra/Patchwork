//! Shared data contract between the Rust backend and the TypeScript frontend.
//! Mirrors `apps/web/src/api.ts` — keep both in sync. All fields serialize as camelCase.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
    /// Saved as "gemini" before the rename; the alias keeps those threads and settings readable.
    #[serde(alias = "gemini")]
    Antigravity,
    OpenCode,
    /// OpenAI-compatible API providers, run by Patchwork's own agent.
    Api,
}

impl AgentKind {
    pub const ALL: [AgentKind; 5] = [
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::Antigravity,
        AgentKind::OpenCode,
        AgentKind::Api,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    #[default]
    Dark,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "camelCase")]
pub struct Integrations {
    pub github: bool,
    pub terminal: bool,
    pub figma: bool,
    pub browser: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub onboarded: bool,
    pub theme: Theme,
    /// Connected agents; the first is the default for new tasks.
    pub agents: Vec<AgentKind>,
    /// Legacy single-agent setting, migrated into `agents` on load.
    #[serde(skip_serializing)]
    pub agent: Option<AgentKind>,
    /// Agent CLIs the user located by hand, used instead of searching for them.
    pub agent_paths: HashMap<AgentKind, String>,
    pub integrations: Integrations,
    pub projects: Vec<Project>,
    pub active_project_id: Option<String>,
    pub tokens: TokenSettings,
    /// Model, `provider/model`, that reads the images of a message on Patchwork's own agent.
    /// None sends images to the main model.
    pub vision_model: Option<String>,
    /// Shell that agents run commands in. None follows the SHELL environment variable.
    pub shell: Option<String>,
    /// Folder of the skills every agent loads. None uses `skills` in the config folder.
    pub skills_dir: Option<String>,
    /// Skills switched off in Settings, by name.
    pub disabled_skills: Vec<String>,
    pub notifications: NotificationSettings,
    pub updates: UpdateSettings,
    /// Id of the app icon color way in `assets/logos/manifest.json`. None uses the manifest default.
    pub logo: Option<String>,
    /// Legacy switch for the import log, migrated into `logging` on load.
    #[serde(skip_serializing)]
    pub import_log_disabled: bool,
    pub logging: LoggingSettings,
    pub terminal: TerminalSettings,
    pub permissions: PermissionSettings,
    /// Answering an agent's question takes a second click on the picked answer.
    pub confirm_answers: bool,
    /// Folder of plugin bundles. None uses `plugins` in the config folder.
    pub plugins_dir: Option<String>,
    /// Plugins switched off in Settings, by folder name.
    pub disabled_plugins: Vec<String>,
    /// Whether the agent may look at the screen and drive the pointer and keyboard.
    pub computer_use: bool,
}
/// What agents may do without asking: for every project and chat, for chats without a project,
/// and for each project.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct PermissionSettings {
    pub everywhere: ScopeRules,
    pub chats: ScopeRules,
    /// By project id.
    pub projects: HashMap<String, ScopeRules>,
    /// Set once the starting rules were added, so deleting one does not bring it back.
    pub seeded: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct ScopeRules {
    /// What each kind of request gets when no rule matches it; a missing kind inherits.
    pub defaults: BTreeMap<PermissionKind, Effect>,
    pub rules: Vec<PermissionRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    pub id: String,
    pub kind: PermissionKind,
    /// A folder, a command's leading words, a web host, or an MCP server; `*` is anything, and
    /// `.` is the thread's own folder.
    pub pattern: String,
    pub effect: Effect,
}

/// The kinds of request a rule can answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum PermissionKind {
    Edit,
    Command,
    Folder,
    Web,
    Mcp,
    /// The screen: a screenshot, or the pointer and keyboard.
    Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Ask,
    Deny,
}

/// Where a permission rule applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "projectId", rename_all = "lowercase")]
pub enum PermissionScope {
    Everywhere,
    Chats,
    Project(String),
}

/// The rule that answered an approval: a rule by id, or the scope's default for the kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuleRef {
    pub scope: PermissionScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
}

/// The integrated terminal, as VS Code's `terminal.integrated.*` settings set it up, with its
/// defaults. The shell is `shell`, shared with the agents.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct TerminalSettings {
    /// Arguments for the shell. None is `-l` on macOS, none elsewhere.
    pub shell_args: Option<Vec<String>>,
    /// Variables added to the shell's environment; null removes one. Values may use
    /// `${env:NAME}` and `${workspaceFolder}`.
    pub env: BTreeMap<String, Option<String>>,
    /// Whether the shell starts from Patchwork's environment, rather than a minimal one.
    pub inherit_env: bool,
    /// The folder shells start in; None is the project's. May use `${workspaceFolder}`.
    pub cwd: Option<String>,
    /// Whether `LANG` is set to a UTF-8 locale: `auto` when it is missing or not UTF-8.
    pub detect_locale: DetectLocale,
    /// None uses the app's monospace font.
    pub font_family: Option<String>,
    pub font_size: f32,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub font_weight: String,
    pub font_weight_bold: String,
    pub cursor_style: CursorStyle,
    pub cursor_blinking: bool,
    /// The cursor's width when `cursor_style` is `line`.
    pub cursor_width: u32,
    pub scrollback: u32,
    pub copy_on_selection: bool,
    pub right_click_behavior: RightClickBehavior,
    pub alt_click_moves_cursor: bool,
    pub mac_option_is_meta: bool,
    pub draw_bold_text_in_bright_colors: bool,
    pub minimum_contrast_ratio: f32,
    pub smooth_scrolling: bool,
    pub mouse_wheel_scroll_sensitivity: f32,
    /// The scroll multiplier while Alt is held.
    pub fast_scroll_sensitivity: f32,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        let mac = cfg!(target_os = "macos");
        Self {
            shell_args: None,
            env: BTreeMap::new(),
            inherit_env: true,
            cwd: None,
            detect_locale: DetectLocale::Auto,
            font_family: None,
            font_size: if mac { 12.0 } else { 14.0 },
            line_height: if cfg!(target_os = "linux") { 1.1 } else { 1.0 },
            letter_spacing: 0.0,
            font_weight: "normal".into(),
            font_weight_bold: "bold".into(),
            cursor_style: CursorStyle::Block,
            cursor_blinking: false,
            cursor_width: 1,
            scrollback: 1000,
            copy_on_selection: false,
            right_click_behavior: if mac {
                RightClickBehavior::SelectWord
            } else if cfg!(windows) {
                RightClickBehavior::CopyPaste
            } else {
                RightClickBehavior::Default
            },
            alt_click_moves_cursor: true,
            mac_option_is_meta: false,
            draw_bold_text_in_bright_colors: true,
            minimum_contrast_ratio: 4.5,
            smooth_scrolling: false,
            mouse_wheel_scroll_sensitivity: 1.0,
            fast_scroll_sensitivity: 5.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum DetectLocale {
    #[default]
    Auto,
    Off,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Block,
    Line,
    Underline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RightClickBehavior {
    #[default]
    Default,
    CopyPaste,
    Paste,
    SelectWord,
    Nothing,
}

/// How much each log in `logs/<launch>/` records, as VS Code sets each of its logs.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct LoggingSettings {
    /// `main.log`: Patchwork's own messages.
    pub app: LogLevel,
    /// `window/renderer.log`: the window's errors and warnings.
    pub window: LogLevel,
    /// `agents/<agent>.log`: each agent's processes and their traffic.
    pub agents: LogLevel,
    /// `imports.log`: each scan and import of saved conversations.
    pub imports: LogLevel,
}

/// How much a log records; each level includes the ones before it. Below Trace, no
/// conversation or file content is written.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS,
)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Off,
    Error,
    Warning,
    #[default]
    Info,
    Debug,
    /// Everything, with content trimmed and secrets removed.
    Trace,
}

/// How Patchwork keeps itself up to date.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateSettings {
    /// Minutes between checks for a new release: 10 to 60, in steps of 10.
    pub check_every_minutes: u32,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            check_every_minutes: 10,
        }
    }
}

/// Desktop notifications shown while Patchwork is not the focused window.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct TokenSettings {
    /// Shows the token counts under each finished reply.
    pub show_usage: bool,
    /// A context above this is flagged, and a cold task this large is asked about first.
    pub warn_above: u64,
    /// Asks before a large task whose prompt cache has expired is resumed.
    pub ask_before_resume: bool,
    /// The context size at which Claude compacts mid-task. 0 leaves Claude's own, near the
    /// model's full window.
    pub claude_compact_window: u64,
    /// Set once the 200k default for `claude_compact_window` replaced the old default, 0, so
    /// choosing 0 again sticks.
    // Missing in settings saved before it, unlike the struct's default.
    #[serde(default)]
    pub compact_window_defaulted: bool,
    /// Model for new tasks, per agent. A missing agent uses the CLI's own default.
    pub default_models: HashMap<AgentKind, String>,
    /// Names a new chat or task with a title its provider writes from the first message,
    /// rather than that message's first line.
    pub generate_titles: bool,
}

impl Default for TokenSettings {
    fn default() -> Self {
        Self {
            show_usage: true,
            warn_above: 150_000,
            ask_before_resume: true,
            claude_compact_window: 200_000,
            compact_window_defaulted: true,
            default_models: HashMap::new(),
            generate_titles: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: Option<String>,
    pub label: String,
    /// Configured CLI default; not a claim about a resumed session's runtime model.
    pub default_model: Option<String>,
    /// Superseded model, listed under the picker's "Legacy models" section.
    pub is_legacy: bool,
    /// The thinking levels this model takes, as its CLI reports them. `None` when the CLI does
    /// not say, so the picker offers the levels its agent shares; an empty list when the model
    /// takes none, so the picker offers no level for it.
    pub efforts: Option<Vec<String>>,
}

/// A slash command advertised by a provider CLI for this thread/workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSlashCommand {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// The shells the Settings dropdown offers, and the one "System default" resolves to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShellChoices {
    pub shells: Vec<String>,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Streaming,
    Complete,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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
}

/// Where a task the agent started stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
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
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub text: String,
    pub status: StepStatus,
}

/// A scheduled task: a prompt that starts a task at a fixed time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub schedule: Schedule,
    /// The project to run in; None starts a chat in a scratch folder.
    pub project_id: Option<String>,
    pub agent: AgentKind,
    pub model: Option<String>,
    pub access_mode: AccessMode,
    pub enabled: bool,
    /// When the task last started, in Unix milliseconds.
    pub last_run_at: Option<i64>,
    /// When it next starts, in Unix milliseconds.
    pub next_run_at: Option<i64>,
    pub created_at: i64,
    /// The newest few runs, newest first.
    #[serde(default)]
    pub recent_runs: Vec<ScheduledRun>,
}

/// One time a scheduled task's slot came round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledRun {
    /// When it started, or for a skipped run the slot it missed, in Unix milliseconds.
    pub at: i64,
    pub thread_id: Option<String>,
    pub outcome: RunOutcome,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum RunOutcome {
    Started,
    Failed,
    /// Its time went by more than a day ago, so it was not run late.
    Skipped,
}

/// How often a scheduled task runs. The shapes Codex's own tasks use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Schedule {
    Hourly {
        interval_hours: u32,
    },
    /// Every day at `time`, `HH:MM` in local time.
    Daily {
        time: String,
    },
    /// Monday to Friday at `time`.
    Weekdays {
        time: String,
    },
    /// On each of `days` at `time`.
    Weekly {
        days: Vec<Weekday>,
        time: String,
    },
}

/// A day of the week, written as Codex's schedules write it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum Weekday {
    #[serde(rename = "MO")]
    Mon,
    #[serde(rename = "TU")]
    Tue,
    #[serde(rename = "WE")]
    Wed,
    #[serde(rename = "TH")]
    Thu,
    #[serde(rename = "FR")]
    Fri,
    #[serde(rename = "SA")]
    Sat,
    #[serde(rename = "SU")]
    Sun,
}

impl Weekday {
    pub fn from_chrono(day: chrono::Weekday) -> Self {
        match day {
            chrono::Weekday::Mon => Weekday::Mon,
            chrono::Weekday::Tue => Weekday::Tue,
            chrono::Weekday::Wed => Weekday::Wed,
            chrono::Weekday::Thu => Weekday::Thu,
            chrono::Weekday::Fri => Weekday::Fri,
            chrono::Weekday::Sat => Weekday::Sat,
            chrono::Weekday::Sun => Weekday::Sun,
        }
    }

    /// `Mon`, `Tue`, … for a list row.
    pub fn short(self) -> &'static str {
        match self {
            Weekday::Mon => "Mon",
            Weekday::Tue => "Tue",
            Weekday::Wed => "Wed",
            Weekday::Thu => "Thu",
            Weekday::Fri => "Fri",
            Weekday::Sat => "Sat",
            Weekday::Sun => "Sun",
        }
    }
}

/// A plugin bundle: a folder with a `plugin.json`, the hooks it runs, and the apps it names.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    /// The folder name, and what `disabled_plugins` records.
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub path: String,
    pub hooks: Vec<PluginHook>,
    pub apps: Vec<PluginApp>,
    pub enabled: bool,
}

/// One handler a plugin runs on a turn event, such as `stop`.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginHook {
    pub event: String,
    pub handler: PluginHookHandler,
}

/// What a hook does. Only `Command` runs today; the rest are listed so the user sees them.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginHookHandler {
    Command {
        command: String,
        timeout_secs: u64,
    },
    McpTool {
        server: String,
        tool: String,
    },
    Prompt,
    Agent,
    /// A handler type this version does not know.
    Unsupported {
        kind: String,
    },
}

/// A connector app a plugin asks for, as Codex lists them.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginApp {
    pub name: String,
    pub id: String,
    pub required: bool,
}

/// How much an agent may do without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, TS)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalStatus {
    Pending,
    Allowed,
    Denied,
}

/// An action the agent asked permission for during a turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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
}

/// A question the agent asks the user, with the answers it offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Image,
    File,
    Folder,
}

/// Something dropped into the composer, sent to the agent with the message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
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
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    /// The policy area, such as `cyber`; None when the agent names none.
    pub category: Option<String>,
    pub explanation: String,
}

/// Tokens a reply used, summed over the model requests it made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, TS)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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
}

/// Snapshots of a project's files as git tree ids, kept alive by refs under `refs/patchwork`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnTrees {
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
    /// Set on a subagent's thread: the thread and call that started it. Such a thread is
    /// read-only and not listed in the sidebar.
    #[serde(default)]
    pub parent: Option<ThreadParent>,
    /// Turns run in plan mode: the agent plans and proposes, without changing files.
    #[serde(default)]
    pub plan_mode: bool,
    /// Provider-native commands discovered from the CLI's initialization frame.
    #[serde(default)]
    pub slash_commands: Vec<ProviderSlashCommand>,
    /// The OpenCode session this thread was imported from, so a re-import does not add it again.
    #[serde(default)]
    pub imported_from: Option<String>,
    /// An imported chat whose transcript the next turn sends as context, since no agent session
    /// holds it yet. Cleared once a turn has started.
    #[serde(default)]
    pub context_pending: bool,
    /// The primary custom agent the chat is in the role of (see `agents`).
    #[serde(default)]
    pub role: Option<String>,
    /// The role changed since the agent was last told of it, by hand rather than by summoning.
    #[serde(skip)]
    pub role_pending: bool,
}

/// Where a subagent's thread came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadParent {
    pub thread_id: String,
    pub tool_id: String,
}

/// A folder inside a project. Its threads are those whose `folder_id` names it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub project_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    /// "modified" | "added" | "deleted" | "untracked" | "renamed"
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub staged: bool,
}

/// A settled burst of file changes in the watched project, with git's state after it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectChanged {
    pub project_id: String,
    /// Project-relative paths the burst touched; `.git/...` for commits, staging and checkouts.
    pub paths: Vec<String>,
    pub branch: Option<String>,
    /// None when `git status` failed, so the last good list stays.
    pub changes: Option<Vec<FileChange>>,
}

/// One rate-limit window, e.g. the 5-hour session limit.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub label: String,
    /// 0–100.
    pub used_percent: f64,
    /// Unix epoch milliseconds.
    pub resets_at: Option<u64>,
}

/// Subscription usage limits for one agent, as reported by its provider.
#[derive(Debug, Clone, Serialize, TS)]
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
/// files, or another app (OpenCode, for one) that ran those agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ImportSource {
    Claude,
    Codex,
    #[serde(alias = "gemini")]
    Antigravity,
    T3,
    OpenCode,
}

/// Conversations a source has saved on disk that can be imported.
#[derive(Debug, Clone, Serialize, TS)]
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
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentEventEnvelope {
    pub thread_id: String,
    pub event: AgentEvent,
}

#[derive(Debug, Clone, Serialize, TS)]
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
    /// Part of the agent's reasoning. Streamed pieces carry no times; the saved block
    /// carries when it started and ended.
    Thinking {
        message_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        started_at: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ended_at: Option<u64>,
    },
    ToolStarted {
        message_id: String,
        tool: ToolCall,
    },
    ToolFinished {
        message_id: String,
        tool_id: String,
        is_error: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
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
        /// The user's answers to the agent's questions, by question id.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        answers: HashMap<String, Vec<String>>,
        /// The rule that answered it, or that the user saved when answering it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rule: Option<RuleRef>,
        /// Answered by a rule, without asking.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        auto: bool,
    },
    /// The thread's goal changed; None once it is cleared.
    GoalUpdated {
        goal: Option<Goal>,
    },
    /// The reply ended and the agent is checking whether it met the goal (Claude). The
    /// message's next output ends the check.
    GoalChecking {
        message_id: String,
    },
    /// The agent's model request failed or stalled and the agent is retrying it. Streamed, not
    /// saved; the message's next output ends it.
    Retrying {
        message_id: String,
        reason: String,
        attempt: u32,
        max_attempts: u32,
    },
    /// Provider-native slash command names advertised by the active CLI session.
    SlashCommandsUpdated {
        commands: Vec<ProviderSlashCommand>,
    },
    /// The chat's custom agent changed (see `Thread::role`).
    RoleChanged {
        role: Option<String>,
    },
    /// The chat switched to a custom agent's model.
    ModelChanged {
        model: Option<String>,
    },
    /// The tokens a reply has used so far; replaces the count it had.
    Usage {
        message_id: String,
        usage: TokenUsage,
    },
    /// In plan mode, the agent proposed this plan (Markdown).
    PlanProposed {
        message_id: String,
        text: String,
    },
    /// A steering message cut the reply short.
    MessageCutOff {
        message_id: String,
    },
    /// A safety classifier blocked the reply, or part of it.
    Refused {
        message_id: String,
        refusal: Refusal,
    },
    /// A message sent mid-turn is now waiting for the turn to end (`queued`), or no longer is.
    MessageQueued {
        message_id: String,
        queued: bool,
    },
    /// A queued message was promoted into the running turn.
    MessageSteered {
        message_id: String,
    },
    /// A queued message was taken back, so it leaves the thread.
    MessageRemoved {
        message_id: String,
    },
    /// A queued message was edited in place.
    MessageEdited {
        message: Message,
    },
    /// A task the agent left running past its reply is listed under the message it started in.
    BackgroundTaskStarted {
        message_id: String,
        task: BackgroundTask,
    },
    /// A listed task changed: its status, description, or progress.
    BackgroundTaskUpdated {
        message_id: String,
        task: BackgroundTask,
    },
    /// A listed task ended, with its final status.
    BackgroundTaskEnded {
        message_id: String,
        task: BackgroundTask,
    },
    /// The project's snapshots from before and after the turn that wrote the message.
    TurnRecorded {
        message_id: String,
        turn: TurnTrees,
    },
    /// Terminal event for a turn. `status` is complete | error | cancelled.
    TurnCompleted {
        message_id: String,
        status: MessageStatus,
        error: Option<String>,
    },
    /// A thread the server created, such as a subagent's.
    ThreadCreated {
        thread: Thread,
    },
    /// The thread was named after what its first message is about.
    TitleUpdated {
        title: String,
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

/// A feature flag of the installed Codex CLI, from `codex features list`.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CodexFeature {
    pub name: String,
    /// `stable`, `beta`, `under development`, `deprecated`, or `removed`.
    pub stage: String,
    pub enabled: bool,
}

/// A model in the installed Codex CLI's catalog, from `codex debug models`.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CodexModel {
    pub slug: String,
    pub display_name: String,
    pub context_window: Option<u64>,
    pub efforts: Vec<String>,
    pub speed_tiers: Vec<String>,
    pub multi_agent: Option<String>,
}

/// What the installed Codex CLI offers: its version, flags, and models.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CodexInfo {
    pub path: String,
    pub version: String,
    pub features: Vec<CodexFeature>,
    pub models: Vec<CodexModel>,
}

/// A feature gate of the installed Claude Code, as the CLI last cached it in `.claude.json`.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeFeature {
    /// The gate's name, such as `tengu_bridge_poll_interval_ms`.
    pub name: String,
    /// The cached value as one line of JSON, shortened when it is long.
    pub value: String,
    /// Present when the gate is a plain switch. A gate that carries a setting, such as a poll
    /// interval or a variant name, has no on or off, so this is `None`.
    pub enabled: Option<bool>,
}

/// A model in the catalog Claude Code cached for the terminal.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeModel {
    pub id: String,
    pub name: String,
    pub context_window: Option<u64>,
    pub efforts: Vec<String>,
    /// Whether the model offers fast mode.
    pub fast_mode: bool,
}

/// What the installed Claude Code offers: its path and version, the feature gates it has cached,
/// and the models in its catalog.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeInfo {
    pub path: String,
    pub version: String,
    pub features: Vec<ClaudeFeature>,
    pub models: Vec<ClaudeModel>,
}
