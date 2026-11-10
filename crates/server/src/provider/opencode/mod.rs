//! OpenCode CLI (`opencode run --format json`), versions 1.x and 2.x. Each turn is one run that
//! prints a JSON event per line and exits; nothing is written to its stdin.

// Rust guideline compliant 2026-10-09

mod config;
mod run;
mod serve;

#[doc(inline)]
pub use config::{
    opencode_bin, auth, models, permission_config, skills_config, agents_config, config_content,
    instructions_config, todo_config,
};
use config::major_version;
#[doc(inline)]
pub use run::{turn_args, parse};
use run::{todo_plan, tool_name};
#[doc(inline)]
pub use serve::{uses_serve, bridge_request, mode_asks, steer_line, approval_reply};
use serve::{action, parse_serve};

pub mod bridge;
pub mod cli;

#[cfg(test)]
mod tests;
