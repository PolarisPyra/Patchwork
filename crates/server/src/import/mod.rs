//! Imports conversations that the agent CLIs saved on disk, so work started
//! outside Patchwork shows up as threads. Each imported thread keeps the CLI's
//! session id, so follow-up messages resume the original conversation.

mod ops;
mod types;

pub use types::{ImportScan, ImportSource};

mod antigravity;
mod claude;
mod codex;
mod opencode;
mod t3;

mod sessions;

pub use sessions::*;
pub(crate) use sessions::ilog;
