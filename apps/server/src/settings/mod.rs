//! The app's settings: their shape, and saving them.

mod ops;
mod types;

pub use types::{
    CursorStyle, DetectLocale, Integrations, LoggingSettings, LogLevel, NotificationSettings,
    RightClickBehavior, Settings, ShellChoices, TerminalSettings, Theme, TokenSettings,
    UpdateSettings,
};
