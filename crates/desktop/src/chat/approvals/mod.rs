//! A reply's approvals and questions, and the permission
//! request the agent is waiting on, above the composer.

// Rust guideline compliant 2026-10-09

mod card;
mod answers;
mod banner;

pub use card::{AllowChoice, approvals};
