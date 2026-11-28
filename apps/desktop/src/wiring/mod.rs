//! What makes one area answer another's events: each screen the window holds, made with its
//! view of the window's state and what it asks of the window.

// Rust guideline compliant 2026-10-09

mod agents;
mod android;
pub(crate) mod background;
mod canvas;
mod chat;
mod granular;
mod onboarding;
mod orchestrator;
mod projects;
mod quilts;
pub(crate) mod search;
mod sidebar;
mod source_control;
mod usage;
