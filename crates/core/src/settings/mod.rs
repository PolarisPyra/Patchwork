//! The app's settings: their shape ([`types`]), and saving them ([`save`]).

// Rust guideline compliant 2026-10-09

mod save;
mod types;

pub(crate) use save::follow;
#[doc(inline)]
pub use types::{
    CheckSettings, CursorStyle, DetectLocale, Integrations, LoggingSettings, LogLevel,
    NotificationSettings, ResumeAtLaunch, RightClickBehavior, Settings, TerminalSettings, TextSize,
    Theme, TokenSettings, UpdateSettings, VisionModel,
};
