//! Agent CLIs: locating and probing them, building turn arguments, and mapping
//! their JSONL output. Each provider module knows one CLI.

mod locate;
mod parsed;
mod probe;
mod turn;

pub use locate::{set_agent_paths, codex_binary, agent_command, user_command, turn_command, shell_var};
use locate::{located, located_or, find_binary};
pub use parsed::{TaskEvent, Parsed, GoalUpdate};
pub(crate) use parsed::{one_line};
use parsed::{
    relative, ToolStart, tool_started, tool_finished, thinking, file_touched, text_delta, summarize,
};
pub use probe::{Probe, probe, probe_until, probe_in, detect_agent, cli_version, list_models};
pub(crate) use probe::{run_probe};
use probe::{PROBE_TIMEOUT, stop, read_pipe, read_pipe_upto, text};
pub use turn::{
    GoalCommand, goal_argument, goal_command, TurnSpec, Image, load_images, prompt, Prompt,
    steer_input, goal_pause, interrupt, stop_subagent, is_goal_command, turn_args, turn_env,
    turn_input, parse_line, approval_reply,
};
pub(crate) use turn::{without_attachments};
use turn::prompt_text;

mod install;
mod types;

pub use types::{
    AgentKind, AgentStatus, ClaudeFeature, ClaudeInfo, ClaudeModel, CodexFeature, CodexInfo,
    CodexModel, ModelOption, ProviderSlashCommand,
};

pub mod claude;
pub mod cli_info;
pub mod hooks;
pub mod codex;
pub mod antigravity;
pub mod opencode;
pub mod api;
pub mod mcp;
pub mod model_manifest;
