//! The app's settings, as saved and as the Settings window edits them.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::permissions::PermissionSettings;
use crate::projects::Project;
use crate::provider::AgentKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    #[default]
    Dark,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Integrations {
    pub github: bool,
    pub terminal: bool,
    pub figma: bool,
    pub browser: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[ts(export)]
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
    /// What each agent CLI is told at launch after Patchwork's own instructions. API providers
    /// keep theirs with the provider.
    pub system_prompts: HashMap<AgentKind, String>,
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

/// The integrated terminal, as VS Code's `terminal.integrated.*` settings set it up, with its
/// defaults. The shell is `shell`, shared with the agents.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
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
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum DetectLocale {
    #[default]
    Auto,
    Off,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Block,
    Line,
    Underline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
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
#[ts(export)]
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
#[ts(export)]
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
#[ts(export)]
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
#[ts(export)]
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
#[ts(export)]
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
    /// The context size at which Patchwork's own agent compacts an API task before its next
    /// request. 0 leaves it until the provider refuses the conversation as too long.
    pub api_compact_window: u64,
    /// The context size at which Codex compacts mid-task. 0 leaves Codex's own.
    pub codex_compact_window: u64,
    /// The context size at which OpenCode compacts mid-task. 0 leaves OpenCode's own, near the
    /// model's full window.
    pub opencode_compact_window: u64,
    /// The context size over which an Antigravity task compacts before its next message, since
    /// Antigravity has no setting for it. 0 leaves Antigravity's own.
    pub antigravity_compact_window: u64,
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
            api_compact_window: 128_000,
            codex_compact_window: 0,
            opencode_compact_window: 0,
            antigravity_compact_window: 0,
            default_models: HashMap::new(),
            generate_titles: true,
        }
    }
}

impl TokenSettings {
    /// The context size at which `agent` compacts by default; 0 leaves it to the agent.
    pub fn compact_window(&self, agent: AgentKind) -> u64 {
        match agent {
            AgentKind::Claude => self.claude_compact_window,
            AgentKind::Codex => self.codex_compact_window,
            AgentKind::Antigravity => self.antigravity_compact_window,
            AgentKind::OpenCode => self.opencode_compact_window,
            AgentKind::Api => self.api_compact_window,
        }
    }
}

/// The shells the Settings dropdown offers, and the one "System default" resolves to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ShellChoices {
    pub shells: Vec<String>,
    pub default: Option<String>,
}
