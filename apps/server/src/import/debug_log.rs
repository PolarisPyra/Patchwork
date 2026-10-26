//! The import debug log, `<config>/logs/imports.log`: where each scan looked, what it found,
//! what it skipped and why, and what an import added. Turned off in Settings → Imports.

use crate::debug_log::DebugLog;

pub static LOG: DebugLog = DebugLog::new(2_000_000);

/// `format!`s a line into the import log.
macro_rules! ilog {
    ($($arg:tt)*) => {
        $crate::import::debug_log::LOG.write(&format!($($arg)*))
    };
}
pub(crate) use ilog;
