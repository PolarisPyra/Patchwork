//! OpenCode CLI (`opencode run --format json`), versions 1.x and 2.x. Each turn is one run that
//! prints a JSON event per line and exits; nothing is written to its stdin.

// Rust guideline compliant 2026-10-09

mod agent;
pub(crate) use agent::OpenCode;
mod config;
mod requests;
mod run;
mod server_events;

#[doc(inline)]
pub use config::{
    agents_config, config_content, instructions_config, permission_config, skills_config,
    todo_config,
};
#[doc(inline)]
pub use patchwork_provider_opencode::{auth, bridge, cli, models, opencode_bin};
pub(super) use patchwork_provider_opencode::needs_own_database;
pub(crate) use patchwork_provider_opencode::{SHARED_DATABASE, V1_DATABASE, data_dir};
use patchwork_provider_opencode::major_version;
#[doc(inline)]
pub use requests::{approval_reply, bridge_request, interrupt_line, mode_asks, steer_line, uses_serve};
use requests::action;
#[doc(inline)]
pub use run::{parse, turn_args, turn_input};
use run::{todo_plan, tool_name};
use server_events::parse_serve;
