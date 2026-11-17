//! The tools Patchwork's own agent gives an API model: reading, searching, and changing the
//! project's files, running shell commands, and fetching web pages.

// Rust guideline compliant 2026-10-09

mod describe;
mod dispatch;
mod files;
mod shell;
mod web;

#[doc(inline)]
pub use describe::{definitions, display_name, summary, shown, approval, writes};

#[doc(inline)]
pub use dispatch::{MAX_RESULT, cap, run};
#[doc(inline)]
pub use shell::{BASH_TIMEOUT, MAX_BASH_TIMEOUT, outcome};
pub(crate) use shell::bash_command;
