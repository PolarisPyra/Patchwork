//! API providers as an agent: a turn runs Patchwork's own harness (`crate::harness`) against the
//! provider of the task's model, and this maps the harness's `api.*` lines.

pub mod config;

mod turn;

pub use turn::*;
