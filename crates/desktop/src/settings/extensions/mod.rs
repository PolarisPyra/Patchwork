//! Settings → Extensions: what agents can be given, one
//! kind at a time: custom agents, skills, and hooks. Each tab draws itself in its own file.

// Rust guideline compliant 2026-10-09

mod agents;
mod hooks;
mod skills;
mod view;

pub(super) use crate::settings::controls::list_box;
use view::{AGENT_SIGIL, SKILL_SIGIL, empty, example, filter_chip};
pub use view::{ExtensionsState, render};
