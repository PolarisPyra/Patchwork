//! Debug logs, system snapshot, and per-chat bug reports.
//!
//! Session log files, secret scrubbing, `main.log`, and the per-channel writers live in
//! [`patchwork_daemon::logging`]; this module re-exports them and adds [`system`] snapshots and
//! [`Core::chat_report`] / [`Core::export_logs`].

// Rust guideline compliant 2026-10-09

use std::path::Path;

mod report;
pub mod system;

#[doc(inline)]
pub use patchwork_daemon::logging::{
    AGENT_LOG, CHAT_LOG, ChatLog, Data, DebugLog, IMPORT_LOG, Json, LogChunk, Source, THEME_LOG,
    WINDOW_LOG, channels, cut, export, follow, hide, local_time, log_part, logs_folder, read,
    redact, scrubbed, session, session_dir, set_known, start, start_main_log,
};

use crate::{Core, Error};

impl Core {
    /// Writes the zip [`export`] makes at `dest`, with this launch's system report and the
    /// settings: what Settings' Export logs and an error's Save logs save.
    ///
    /// # Errors
    ///
    /// Returns an error when [`export`] does.
    pub fn export_logs(&self, dest: &Path) -> Result<(), Error> {
        let mut system = system::report().clone();
        system["settings"] = serde_json::json!(self.settings());
        export(dest, &system)
    }
}
