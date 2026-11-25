//! Agent CLIs: locating and probing them, building turn arguments, and mapping
//! their JSONL output. Each provider module knows one CLI.

// Rust guideline compliant 2026-10-09

mod locate;
mod parsed;
mod probe;
mod turn;

#[doc(inline)]
pub use locate::{agent_shell, set_agent_paths, user_command};
pub(crate) use locate::{agent_binary, agent_command, remote_turn_command, turn_command};
use locate::{command_at, located, located_or, find_binary};
#[doc(inline)]
pub use parsed::{TaskEvent, Parsed};
pub(crate) use parsed::{clip, one_line, step_status};
use parsed::{
    relative, ToolStart, tool_started, tool_finished, thinking, file_touched, text_delta,
    summarize, edit_lines, changed_lines,
};
#[doc(inline)]
pub use probe::{Probe, probe, probe_until, probe_in, detect_agent, cli_version, list_models};
use probe::{probe_at, stop};
#[doc(inline)]
pub use turn::{
    GoalCommand, goal_argument, goal_command, TurnSpec, Image, load_images, prompt, Prompt,
    resume_input, steer_input, interrupt, is_goal_command, is_compact_command, turn_args, turn_env,
    withheld_env, turn_input, closes_input, parse_line, approval_reply,
};
pub(crate) use turn::{without_attachments};

mod acp;
pub(crate) use acp::label as acp_label;
mod install;
mod cli_update;
#[doc(inline)]
pub use cli_update::{CliUpdate, UpdateMethod, check_cli_update, update_cli};
mod types;

#[doc(inline)]
pub use types::{
    AgentKind, AgentStatus, ClaudeFeature, ClaudeInfo, ClaudeModel, CliInfo, CliSetting,
    CodexFeature, CodexInfo, CodexModel, ModelOption, ProviderSlashCommand,
};

pub mod claude;
pub mod cli_info;
pub mod hooks;
pub mod command_size;
pub mod codex;
pub mod antigravity;
pub mod opencode;
pub mod api;
pub mod mcp;
