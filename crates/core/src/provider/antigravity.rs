//! Antigravity (`agy acp`, Agent Client Protocol JSON-RPC over stdio).

// Rust guideline compliant 2026-11-22

mod client;
mod input;
mod login;
mod parse;
mod swarm;

#[doc(inline)]
pub use client::{
    antigravity_bin, auth, command, configured_model, home, info, models, parse_acp_models,
    parse_models,
};
pub(crate) use client::tier;
#[doc(inline)]
pub use input::{approval_reply, interrupt, prompt_input, turn_args, turn_env, turn_input};
#[doc(inline)]
pub use login::{LoginSession, sign_out, start_login};
#[doc(inline)]
pub use parse::{background_task, is_work, parse};
#[doc(inline)]
pub use swarm::{ChildTarget, SWARM_TASK_PREFIX, SwarmState, extract_child_target, subagent_events};
