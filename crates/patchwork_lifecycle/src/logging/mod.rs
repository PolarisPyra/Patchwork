//! Debug logs, for people to send with a bug report.
//!
//! Each launch writes a folder, `<config>/logs/<YYYYMMDDTHHMMSS>` in local time, and the newest
//! `KEEP_SESSIONS` are kept. The folder holds only files, so it zips and reads as it is:
//!
//! ```text
//! 20261026T121729/
//! ├─ main.log                 Patchwork's own messages and panics (see `main_log`)
//! ├─ system.json              what Patchwork runs on
//! ├─ imports.log              scans and imports of saved conversations
//! ├─ themes.log               themes and backgrounds added, written, reset and deleted
//! ├─ window.log               the window's errors and warnings
//! ├─ agent-claude.log         one log per agent: its processes and their traffic
//! └─ chat-<id>.log            one file per chat with a warning or an error in this launch, or
//!                             whose Open logs was used
//! ```
//!
//! Each line reads `2026-10-26 12:17:29.562 [info] message`, and each log is set to Trace,
//! Debug, Info, Warning, Error or Off in Settings → General → Logs, or from the Output panel. A full
//! log moves to `<name>.1.log` and so on, keeping six files of up to 5 MB.

// Rust guideline compliant 2026-10-09

mod chat_log;
mod debug_log;
mod files;
mod format;
mod main_log;
mod scrub;

#[doc(inline)]
pub use chat_log::{CHAT_LOG, ChatLog, log_part};
#[doc(inline)]
pub use patchwork_model::logging::{LOG_LEVELS, LogLevel, LoggingSettings};
#[doc(inline)]
pub use debug_log::{
    AGENT_LOG, Data, DebugLog, IMPORT_LOG, Source, THEME_LOG, WINDOW_LOG, follow, start,
};
use debug_log::{LaunchLog, Record, may_report_error};
#[doc(inline)]
pub use files::{LogChunk, channels, export, logs_folder, read, session, session_dir};
#[doc(inline)]
pub use format::local_time;
#[doc(inline)]
pub use main_log::start_main_log;
#[doc(inline)]
pub use scrub::{Json, cut, hide, redact, scrubbed, set_known};
