//! OpenCode CLI (`opencode run --format json`), versions 1.x and 2.x. Each turn is one run that
//! prints a JSON event per line and exits; nothing is written to its stdin.

mod config;
mod run;
mod serve;

pub use config::{
    opencode_bin, auth, models, permission_config, skills_config, agents_config, config_content,
    instructions_config, todo_config,
};
use config::major_version;
pub use run::{turn_args, parse};
use run::{todo_plan, tool_name};
pub use serve::{uses_serve, bridge_request, mode_asks, steer_line, approval_reply};
use serve::{action, parse_serve};

pub mod bridge;

#[cfg(test)]
mod tests;
