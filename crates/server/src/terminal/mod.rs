//! The integrated terminal: the shell it starts, the running shells, each in a pseudo-terminal,
//! and the processes each one owns.
//!
//! A terminal belongs to Patchwork rather than to the window showing it: a window that reloads
//! attaches again and gets the screen back. One no window shows is closed after two hours idle
//! at its prompt, and closing one ends every process it started.

// Rust guideline compliant 2026-10-09

mod activity;
mod launch;
mod pty;
mod range;
mod session;
mod types;

#[doc(inline)]
pub use launch::{Launch, launch, shells};
#[doc(inline)]
pub use range::sweep;
#[doc(inline)]
pub use session::{FrameSink, Terminals};
#[doc(inline)]
pub use types::{AttachedTerminal, TerminalFrame, TerminalInfo, TerminalShell};
