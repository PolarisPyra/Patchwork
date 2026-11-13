//! Agent CLIs: locating and probing them, building turn arguments, and mapping
//! their JSONL output. Each provider module knows one CLI.

// Rust guideline compliant 2026-10-09

mod locate;
mod parsed;
mod probe;
mod turn;

#[doc(inline)]
pub use locate::{agent_shell, set_agent_paths, user_command};
pub(crate) use locate::{agent_command, remote_turn_command, turn_command};
use locate::{located, located_or, find_binary};
#[doc(inline)]
pub use parsed::{TaskEvent, Parsed, GoalUpdate};
pub(crate) use parsed::{one_line, step_status};
use parsed::{
    relative, ToolStart, tool_started, tool_finished, thinking, file_touched, text_delta,
    summarize, edit_lines,
};
#[doc(inline)]
pub use probe::{Probe, probe, probe_until, probe_in, detect_agent, cli_version, list_models};
use probe::stop;
#[doc(inline)]
pub use turn::{
    GoalCommand, goal_argument, goal_command, TurnSpec, Image, load_images, prompt, Prompt,
    steer_input, goal_pause, interrupt, stop_subagent, is_goal_command, turn_args, turn_env,
    turn_input, parse_line, approval_reply,
};
pub(crate) use turn::{without_attachments};
use turn::prompt_text;

mod install;
mod types;

#[doc(inline)]
pub use types::{
    AgentKind, AgentStatus, ClaudeFeature, ClaudeInfo, ClaudeModel, CliInfo, CliSetting,
    CodexFeature, CodexInfo, CodexModel, ModelOption, ProviderSlashCommand,
};

pub mod claude;
pub mod cli_info;
pub mod hooks;
pub mod codex;
pub mod antigravity;
pub mod opencode;
pub mod api;
pub mod mcp;
