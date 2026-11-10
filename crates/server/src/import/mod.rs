//! Imports conversations saved by agent CLIs as Patchwork threads.
//!
//! Imports conversations that the agent CLIs saved on disk, so work started
//! outside Patchwork shows up as threads. Each imported thread keeps the CLI's
//! session id, so follow-up messages resume the original conversation.

// Rust guideline compliant 2026-10-09

mod scan;
mod types;

#[doc(inline)]
pub use types::{ImportScan, ImportSource};

mod antigravity;
mod claude;
mod codex;
mod opencode;
mod t3;

mod sessions;

pub(crate) use sessions::ilog;
#[doc(inline)]
pub use sessions::{Session, SessionMessage, is_chat_folder, sessions, to_thread, transcript};
use sessions::push_merged;
