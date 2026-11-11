//! One file's diff, each change with its Note, Discard and Stage buttons, and the review notes for
//! the agent under the lines they are about.

// Rust guideline compliant 2026-10-09

mod parse;
mod review;
mod view;

pub use review::{Review, review_message, Draft};
pub(in crate::canvas) use parse::{Kind, Parsed, parse};
