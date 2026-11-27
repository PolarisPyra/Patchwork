//! Antigravity CLI discovery, ACP session probing, OAuth login, model catalog, and tool-call state.

// Rust guideline compliant 2026-11-23

pub mod calls;
pub mod cli;
pub mod login;
pub mod models;
pub mod probe;
pub mod tool_fields;

#[doc(inline)]
pub use calls::ToolCalls;
#[doc(inline)]
pub use cli::{MIN_VERSION, antigravity_bin, command, home, info, is_older_than_minimum};
#[doc(inline)]
pub use login::{LoginSession, sign_out, start_login};
#[doc(inline)]
pub use models::{EFFORT_SUFFIXES, models, parse_acp_models, parse_models, strip_effort_suffix};
#[doc(inline)]
pub use probe::{auth, auth_required, client_info, configured_model, known_defaults, tier};
#[doc(inline)]
pub use tool_fields::{arg_str, tool_args, tool_output, tool_paths, tool_summary};
