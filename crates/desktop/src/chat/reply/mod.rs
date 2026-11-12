//! A reply: its steps under a head line, then its files, notices and
//! actions.

// Rust guideline compliant 2026-10-09

mod pieces;
mod frame;
mod steps;

pub use pieces::{is_open, pieces_of, reply_piece};
