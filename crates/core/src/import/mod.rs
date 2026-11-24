//! Imports conversations saved by agent CLIs as Patchwork chats.
//!
//! Imports conversations that the agent CLIs saved on disk, so work started
//! outside Patchwork shows up as chats. Each imported chat keeps the CLI's
//! session id, so follow-up messages resume the original conversation.

// Rust guideline compliant 2026-10-09

mod scan;
mod types;

#[doc(inline)]
pub use types::{ImportScan, ImportSource};

mod agy;
mod antigravity;
mod claude;
mod codex;
mod opencode;
mod t3;

mod sessions;

pub(crate) use sessions::ilog;
#[doc(inline)]
pub use sessions::{Session, SessionMessage, is_chat_folder, sessions, to_chat, transcript};
use sessions::push_merged;
