//! Settings → Extensions: what agents can be given, one
//! kind at a time: custom agents, skills, and hooks. Each tab draws itself in its own file.

// Rust guideline compliant 2026-10-09

mod agents;
mod hooks;
mod skills;
mod tabs;

pub(in crate::settings) use crate::settings::controls::list_box;
use tabs::filter_chip;
use crate::settings::controls::{empty, example};
pub use tabs::{ExtensionsState, render};
