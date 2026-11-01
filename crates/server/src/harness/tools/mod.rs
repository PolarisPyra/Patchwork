//! The tools Patchwork's own agent gives an API model: reading, searching, and changing the
//! project's files, running shell commands, and fetching web pages.

mod describe;
mod dispatch;
mod files;
mod shell;
mod web;

pub use describe::{definitions, display_name, summary, shown, approval, writes};

pub use dispatch::*;
