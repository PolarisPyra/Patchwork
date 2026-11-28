//! What the user adds to their agents: subagents, skills and hooks.
//!
//! Each is a folder of files under Patchwork's config folder, which Settings → Extensions edits
//! and every agent is launched with: [`agents`] for the subagents an agent can hand work to,
//! [`skills`] for the instructions it loads by name, and [`hooks`] for the commands that run
//! around its turns.

// Rust guideline compliant 2026-10-09

pub mod agents;
pub mod hooks;
pub mod skills;
