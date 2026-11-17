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
    CheckSettings, CursorStyle, DetectLocale, Integrations, LoggingSettings, LogLevel,
    ResumeAtLaunch, RightClickBehavior, Settings, TerminalSettings, TextSize, Theme, TokenSettings,
    UpdateSettings, VisionModel,
};
