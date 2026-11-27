//! Lightweight client SDK for applications managed by the Oxidation hot-reloader.
//!
//! Link this crate into the application binary so it can:
//! - Receive structured hot-reload commands (`Quit`, `Restart`, `Reload`, `Errors`, `Fixed`)
//!   over standard input without hand-parsing the wire protocol.
//! - Persist and restore typed JSON state across restarts via `OXIDATION_STATE`.
//! - Hand over open file descriptors (such as pseudo-terminals) across restarts on Unix via
//!   `OXIDATION_FDS` (`SCM_RIGHTS`).

// Rust guideline compliant 2026-10-09

pub mod fds;
pub mod protocol;
pub mod session;

#[doc(inline)]
pub use fds::HeldFds;
#[doc(inline)]
pub use protocol::{Command, FDS_VAR, STATE_VAR, encode_command, parse_command, strip_ansi};
#[doc(inline)]
pub use session::ClientSession;
