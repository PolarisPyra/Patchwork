//! Debug logs, for people to send with a bug report. Each launch writes a folder,
//! `<config>/logs/<YYYYMMDDTHHMMSS>` in local time, and the newest `KEEP_SESSIONS` are kept. The
//! folder holds only files, so it zips and reads as it is:
//!
//! ```text
//! 20261026T121729/
//! ├─ main.log                 Patchwork's own messages (written by the desktop shell)
//! ├─ system.json              what Patchwork runs on (see `system`)
//! ├─ imports.log              scans and imports of saved conversations
//! ├─ window.log               the window's errors and warnings
//! ├─ agent-claude.log         one log per agent: its processes and their traffic
//! └─ chat-<id>.log            one file per chat with a warning or an error in this launch, or
//!                             whose Open logs was used: its log, under the report Open logs
//!                             writes at the top (see `thread_log` and `report`)
//! ```
//!
//! Each line reads `2026-10-26 12:17:29.562 [info] message`, and each log is set to Trace,
//! Debug, Info, Warning, Error or Off in Settings → General → Logs, or from the Output panel. A full
//! log moves to `<name>.1.log` and so on, keeping six files of up to 5 MB.
//!
//! For agents, Info has each process's start, CLI version and exit; Warning, output nothing
//! handled; Debug, every line as an outline (its fields, types, ids and numbers, with other
//! strings replaced by their length); and Trace, every line in full. A line reporting an error
//! is logged in full from Error up. Logged text is trimmed: secrets are replaced (values under
//! secret-looking keys, and anything that looks like a key, token, password or private key)
//! and long strings are cut short.
//!
//! Records go to a writer thread, so a turn reading its agent's output never waits on the
//! disk. The thread writes each burst of records out as soon as it arrives, so a killed app
//! loses at most the records still queued.

mod debug_log;
mod files;
mod render;
mod report;
mod scrub;
pub mod system;
mod thread_log;

pub use debug_log::*;
use files::session;
pub use files::{session_dir, channels, LogChunk, read, export};
pub use render::{now_text};
pub use scrub::{Json};
pub use thread_log::{THREAD_LOG, ThreadLog};
