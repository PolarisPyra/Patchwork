//! The agents and providers: which agents there are, what each offers, and the saved API
//! providers.

// Rust guideline compliant 2026-11-27

pub mod api;
mod commands;
mod identity;
mod schema;

#[doc(inline)]
pub use commands::{GoalCommand, goal_argument, goal_command, is_compact_command, is_goal_command};
#[doc(inline)]
pub use identity::{Capabilities, DISPLAY_ORDER, Identity, Logo, ShellChoice, SkillCall, Steer};
#[doc(inline)]
pub use schema::{
    AgentKind, AgentStatus, ClaudeFeature, ClaudeInfo, ClaudeModel, CliInfo, CliSetting,
    CodexFeature, CodexInfo, CodexModel, ModelOption, ProviderSlashCommand,
};
