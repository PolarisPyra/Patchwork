//! Debug logs in `<config>/logs` that people can send with a bug report. Each one is turned
//! off in Settings.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::time::format_rfc3339_ms;
use crate::types::now_ms;

/// `logs/agents.log`: every agent process a turn starts, what it was sent, what it printed,
/// and how it ended. Turned off in Settings → Agents.
pub static AGENTS: DebugLog = DebugLog::new(10_000_000);

pub struct DebugLog {
    path: Mutex<Option<PathBuf>>,
    enabled: AtomicBool,
    /// A log larger than this moves to `<name>.1` and starts over, so it never grows
    /// without bound but still keeps what led up to the move.
    max_bytes: u64,
}

impl DebugLog {
    pub const fn new(max_bytes: u64) -> Self {
        Self { path: Mutex::new(None), enabled: AtomicBool::new(false), max_bytes }
    }

    /// Sets where the log is written. Called once when the server opens.
    pub fn init(&self, path: PathBuf) {
        *self.path.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    }

    /// Follows the setting.
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Appends one timestamped line. Failures to write are ignored: the log is only for debugging.
    pub fn write(&self, line: &str) {
        if !self.enabled() {
            return;
        }
        let guard = self.path.lock().unwrap_or_else(|e| e.into_inner());
        let Some(path) = guard.as_ref() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::metadata(path).is_ok_and(|m| m.len() > self.max_bytes) {
            let mut old = path.clone().into_os_string();
            old.push(".1");
            let _ = std::fs::rename(path, old);
        }
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path);
        if let Ok(mut file) = file {
            let _ = writeln!(file, "{} {line}", format_rfc3339_ms(now_ms()));
        }
    }
}

/// `format!`s a line into the agent log, skipping the formatting while the log is off.
macro_rules! alog {
    ($($arg:tt)*) => {
        if $crate::debug_log::AGENTS.enabled() {
            $crate::debug_log::AGENTS.write(&format!($($arg)*))
        }
    };
}
pub(crate) use alog;
