//! Agent CLIs: locating and probing them, building turn arguments, and mapping
//! their JSONL output. Each provider module knows one CLI.

// Rust guideline compliant 2026-10-09

mod agent;
mod line_reader;
mod locate;
mod parsed;
mod probe;
mod turn;

#[doc(inline)]
pub use agent::{
    AgentProvider, Capabilities, DISPLAY_ORDER, Identity, Logo, ProviderKind, ShellChoice,
    SkillCall, Steer, TurnLaunch, providers,
};
#[doc(inline)]
pub use line_reader::{LineReader, Route};
#[doc(inline)]
pub use locate::{agent_shell, set_agent_paths, user_command};
pub(crate) use locate::{agent_binary, agent_command, remote_turn_command, turn_command};
use locate::located;
#[doc(inline)]
pub use parsed::{Parsed, TaskEvent};
pub(crate) use parsed::{clip, one_line, step_status};
use parsed::{
    ToolStart, changed_lines, edit_lines, file_touched, relative, summarize, text_delta, thinking,
    tool_finished, tool_started,
};
#[doc(inline)]
pub use probe::{Probe, cli_version, detect_agent, list_models, probe, probe_in, probe_until};
use probe::stop;
#[doc(inline)]
pub use turn::{
    GoalCommand, Image, Prompt, TurnSpec, approval_reply, closes_input, goal_argument,
    goal_command, interrupt, is_compact_command, is_goal_command, load_images, parse_line, prompt,
    resume_input, steer_input, turn_args, turn_env, turn_input, withheld_env,
};
pub(crate) use turn::without_attachments;

mod acp;
mod cli_update;
mod install;
#[doc(inline)]
pub use cli_update::{CliUpdate, UpdateMethod, check_cli_update, update_cli};

#[doc(inline)]
pub use patchwork_provider_core::{
    AgentKind, AgentStatus, ClaudeFeature, ClaudeInfo, ClaudeModel, CliInfo, CliSetting,
    CodexFeature, CodexInfo, CodexModel, ModelOption, ProviderSlashCommand, cli_info, command_size,
};

pub mod antigravity;
pub mod api;
pub mod claude;
pub mod codex;
pub mod hooks;
pub mod mcp;
pub mod opencode;
