//! The app's settings: their shape ([`types`]), saving them ([`save`]), and keeping the
//! terminal variables' values in the vault ([`seal`]).

// Rust guideline compliant 2026-10-09

mod save;
mod seal;
mod types;

pub(crate) use save::follow;
pub(crate) use seal::{forget, unseal};
#[doc(inline)]
pub use crate::notifications::NotificationSettings;
#[doc(inline)]
pub use types::{
    CheckSettings, CursorStyle, DetectLocale, Integrations, LoggingSettings, LogLevel, LOG_LEVELS,
    ResumeAtLaunch, RightClickBehavior, Settings, TerminalSettings, Theme, TokenSettings,
    UpdateSettings, VisionModel, KeepWorktrees, WorktreeCleanup, WorktreeSettings, WindowState,
};
