//! The import debug log, `<config>/logs/imports.log`: where each scan looked, what it found,
//! what it skipped and why, and what an import added. Turned off in Settings → Imports.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::time::format_rfc3339_ms;
use crate::types::now_ms;

/// A log larger than this is started over, so it never grows without bound.
const MAX_BYTES: u64 = 2_000_000;

static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Sets where the log is written. Called once when the server opens.
pub fn init(path: PathBuf) {
    *PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
}

/// Follows the setting; checked at the start of every scan and import.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Appends one timestamped line. Failures to write are ignored: the log is only for debugging.
pub fn write(line: &str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let guard = PATH.lock().unwrap_or_else(|e| e.into_inner());
    let Some(path) = guard.as_ref() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let too_big = std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(!too_big)
        .write(true)
        .truncate(too_big)
        .open(path);
    if let Ok(mut file) = file {
        let _ = writeln!(file, "{} {line}", format_rfc3339_ms(now_ms()));
    }
}

/// `format!`s a line into the import log.
macro_rules! ilog {
    ($($arg:tt)*) => {
        $crate::import::debug_log::write(&format!($($arg)*))
    };
}
pub(crate) use ilog;
