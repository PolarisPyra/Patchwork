//! A reply's approvals and questions. The banner for the request the agent waits on is
//! `above_composer/approval_banner.rs`.

// Rust guideline compliant 2026-10-09

mod answers;
pub(crate) mod card;

pub use card::{AllowChoice, approvals};
