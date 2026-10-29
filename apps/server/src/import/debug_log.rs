//! The import debug log, `imports.log` in the session's log folder: where each scan looked, what it found,
//! what it skipped and why, and what an import added. Set in Settings → Logging.

use crate::logging::DebugLog;

pub static LOG: DebugLog = DebugLog::new("imports.log");

/// `format!`s a line into the import log.
macro_rules! ilog {
    ($($arg:tt)*) => {
        $crate::import::debug_log::LOG.write(
            $crate::settings::LogLevel::Info,
            "",
            &$crate::logging::Source::default(),
            $crate::logging::Data::Text(format!($($arg)*)),
        )
    };
}
pub(crate) use ilog;
