//! Logging level and per-channel verbosity settings.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

/// How much each log in `logs/<launch>/` records.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LoggingSettings {
    /// `main.log`: Patchwork's own messages.
    pub app: LogLevel,
    /// `window.log`: the window's errors and warnings.
    pub window: LogLevel,
    /// `agent-<agent>.log`: each agent's processes and their traffic.
    pub agents: LogLevel,
    /// `imports.log`: each scan and import of saved conversations.
    pub imports: LogLevel,
    /// `themes.log`: each theme and background added, written, reset or deleted.
    pub themes: LogLevel,
    /// `chat-<id>.log`: the events and agent traffic of each chat with a warning or an error.
    pub chats: LogLevel,
    /// A chat's right-click menu has Open logs.
    pub chat_menu: bool,
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            app: LogLevel::Info,
            window: LogLevel::Info,
            agents: LogLevel::Info,
            imports: LogLevel::Info,
            themes: LogLevel::Info,
            // A chat's log is what gets sent when something goes wrong in it, so it keeps what
            // each tool call and error said.
            chats: LogLevel::Debug,
            chat_menu: true,
        }
    }
}

/// How much a log records; each level includes the ones before it. Below Trace, no
/// conversation or file content is written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
