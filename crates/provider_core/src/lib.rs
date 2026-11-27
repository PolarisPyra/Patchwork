//! Shared agent provider primitives: schema, capabilities, CLI discovery, probing, and diagnostics.

// Rust guideline compliant 2026-11-26

pub mod cli_info;
pub mod command_size;
mod identity;
pub mod locate;
pub mod probe;
mod schema;
pub mod text;
pub mod version;

#[doc(inline)]
pub use identity::{Capabilities, DISPLAY_ORDER, Identity, Logo, ShellChoice, SkillCall, Steer};
#[doc(inline)]
pub use locate::{
    agent_shell_for_choice, command_at, find_binary, located, located_or, set_agent_paths,
    user_command,
};
#[doc(inline)]
pub use probe::{PROBE_TIMEOUT, Probe, probe_at, probe_command, stop, version_in};
#[doc(inline)]
pub use schema::{
    AgentKind, AgentStatus, ClaudeFeature, ClaudeInfo, ClaudeModel, CliInfo, CliSetting,
    CodexFeature, CodexInfo, CodexModel, ModelOption, ProviderSlashCommand,
};
#[doc(inline)]
pub use text::{clip, one_line, relative, summarize};
#[doc(inline)]
pub use version::{is_older, version_parts};
