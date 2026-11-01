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

use crate::logging::DebugLog;

/// The import debug log, `imports.log` in the session's log folder: where each scan looked, what it found,
/// what it skipped and why, and what an import added. Set in Settings → Logging.
pub static LOG: DebugLog = DebugLog::new("imports.log");

/// `format!`s a line into the import log.
macro_rules! ilog {
    ($($arg:tt)*) => {
        $crate::import::LOG.write(
            $crate::settings::LogLevel::Info,
            "",
            &$crate::logging::Source::default(),
            $crate::logging::Data::Text(format!($($arg)*)),
        )
    };
}
pub(crate) use ilog;
