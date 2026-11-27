//! Settings → Agents, Skills and Hooks: what agents can be given, one kind to a page. Each page
//! draws itself in its own file.

// Rust guideline compliant 2026-10-09

mod agents;
mod hooks;
mod skills;
mod tabs;

pub(in crate::settings) use crate::settings::controls::list_box;
pub(in crate::settings) use tabs::filter_select;
use crate::settings::controls::{empty, example};
pub use tabs::{ExtensionsState, render};
pub(in crate::settings) use tabs::Tab;
