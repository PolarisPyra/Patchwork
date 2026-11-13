//! The device's log, kept to the lines of the project's app: streamed into the panel while it is
//! open, and read once for an agent.
//!
//! Android gives each start of an app a new process, and `logcat --pid` follows only one, so the
//! stream asks the device for the app's process every moment and follows the new one when it
//! changes. An app that crashed has no process: its last words are in the log's crash buffer.

// Rust guideline compliant 2026-10-09

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::device::adb;
use super::devices::ADB_LIMIT;
use super::gradle::application_id;
use super::tool::tool_command;
use super::types::{LogLevel, LogLine, MobileProject, Sdk, Variant};
use crate::{Error, Server};

/// The most lines the panel keeps, the oldest dropped first: a few minutes of a busy app.
const KEPT: usize = 5000;

/// How often the device is asked which process the app runs in.
const PID_POLL: Duration = Duration::from_millis(1500);

/// How many of the device's last lines a log starts with when it is not kept to one app.
const DEVICE_BACKLOG: &str = "200";

/// The most lines an agent is given in one read, which is what fits in its context.
const AGENT_LINES: u64 = 2000;

/// The app's process on the device, when it is running.
fn pid_of(sdk: &Sdk, serial: &str, package: &str) -> Option<u32> {
    let out = adb(sdk, serial, &["shell", "pidof", "-s", package], ADB_LIMIT).ok()?;
    out.stdout.trim().parse().ok()
}

/// One line of the log as `-v threadtime` writes it, such as
/// `11-13 15:36:30.123  1234  1250 E AndroidRuntime: FATAL EXCEPTION: main`.
pub(super) fn line(text: &str) -> LogLine {
    let level = text
        .split_whitespace()
        .nth(4)
        .and_then(|letter| match letter {
            "V" => Some(LogLevel::Verbose),
            "D" => Some(LogLevel::Debug),
            "I" => Some(LogLevel::Info),
            "W" => Some(LogLevel::Warn),
            "E" => Some(LogLevel::Error),
            "F" => Some(LogLevel::Fatal),
            _ => None,
        });
    LogLine {
        level,
        text: text.to_string(),
    }
}

/// The lines read so far.
#[derive(Debug, Default)]
struct Lines {
    kept: VecDeque<LogLine>,
    /// Goes up each time the lines change.
    revision: u64,
}

#[derive(Debug, Default)]
struct Shared {
    lines: Mutex<Lines>,
    /// The `adb logcat` running now.
    child: Mutex<Option<Child>>,
    closed: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    fn push(&self, line: LogLine) {
        let mut lines = lock(&self.lines);
        if lines.kept.len() == KEPT {
            lines.kept.pop_front();
        }
        lines.kept.push_back(line);
        lines.revision += 1;
    }

    /// A line of Patchwork's own, between the device's.
    fn note(&self, text: String) {
        self.push(LogLine { level: None, text });
    }

    fn end_child(&self) {
        if let Some(mut child) = lock(&self.child).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Starts `adb logcat` with `args` in place of the one running, its lines read on a thread
    /// that ends when it does.
    fn follow(self: &Arc<Self>, sdk: &Sdk, serial: &str, args: &[&str]) -> Result<(), Error> {
        self.end_child();
        let mut cmd = tool_command(sdk, sdk.adb()?);
        cmd.args(["-s", serial, "logcat", "-v", "threadtime"])
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take();
        *lock(&self.child) = Some(child);
        // The log may have been closed while adb started.
        if self.closed.load(Ordering::Relaxed) {
            self.end_child();
        }
        let shared = Arc::clone(self);
        std::thread::Builder::new()
            .name(format!("logcat {serial}"))
            .spawn(move || {
                let Some(stdout) = stdout else { return };
                let mut reader = BufReader::new(stdout);
                let mut bytes = Vec::new();
                // A log line may hold bytes that are not text, which must not end the reading.
                while reader.read_until(b'\n', &mut bytes).is_ok_and(|n| n > 0) {
                    let text = String::from_utf8_lossy(&bytes);
                    let text = text.trim_end();
                    if !text.is_empty() {
                        shared.push(line(text));
                    }
                    bytes.clear();
                }
            })?;
        Ok(())
    }
}

/// A device's log as it is written, until this is dropped.
#[derive(Debug)]
pub struct Logcat {
    shared: Arc<Shared>,
    package: Option<String>,
}

impl Logcat {
    /// The lines kept and their revision, when they changed since the revision `seen`.
    #[must_use]
    pub fn lines(&self, seen: Option<u64>) -> Option<(Vec<LogLine>, u64)> {
        let lines = lock(&self.shared.lines);
        (seen != Some(lines.revision))
            .then(|| (lines.kept.iter().cloned().collect(), lines.revision))
    }

    /// Forgets the lines read so far.
    pub fn clear(&self) {
        let mut lines = lock(&self.shared.lines);
        lines.kept.clear();
        lines.revision += 1;
    }

    /// The app the log is kept to. None for the whole device's log.
    #[must_use]
    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }
}

impl Drop for Logcat {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Relaxed);
        self.shared.end_child();
    }
}

/// Follows the app `package` on `serial` from one process to the next until `shared` is closed.
fn follow_app(shared: &Arc<Shared>, sdk: &Sdk, serial: &str, package: &str) {
    let mut following = None;
    let mut waiting = false;
    while !shared.closed.load(Ordering::Relaxed) {
        let pid = pid_of(sdk, serial, package);
        if pid != following {
            following = pid;
            if let Some(pid) = pid {
                waiting = false;
                shared.note(format!("{package} is running as process {pid}"));
                if let Err(e) = shared.follow(sdk, serial, &[&format!("--pid={pid}")]) {
                    shared.note(format!("The log could not be read: {e}"));
                    return;
                }
            } else {
                shared.end_child();
                shared.note(format!("{package} stopped"));
            }
        } else if pid.is_none() && !waiting {
            waiting = true;
            shared.note(format!("Waiting for {package} to start"));
        }
        std::thread::sleep(PID_POLL);
    }
}

/// The log's last `lines` lines on `serial`, for an agent. Kept to the app `package` unless
/// `whole`, and to lines at or above `level`, one of logcat's letters.
///
/// # Errors
///
/// Returns an error when adb fails.
pub(super) fn recent(
    sdk: &Sdk,
    serial: &str,
    package: Option<&str>,
    lines: u64,
    level: Option<char>,
    whole: bool,
) -> Result<String, Error> {
    let count = lines.clamp(1, AGENT_LINES).to_string();
    let mut args = vec!["logcat", "-d", "-v", "threadtime", "-t", &count];
    let pid = package
        .filter(|_| !whole)
        .map(|package| (package, pid_of(sdk, serial, package)));
    let heading;
    let pid_arg;
    match pid {
        Some((package, Some(pid))) => {
            heading = format!("{package} (process {pid}) on {serial}:");
            pid_arg = format!("--pid={pid}");
            args.push(&pid_arg);
        }
        // An app that crashed has no process, and what it said last is in the crash buffer.
        Some((package, None)) => {
            heading = format!(
                "{package} is not running on {serial}. The device's latest crashes, of any app:"
            );
            args.extend(["-b", "crash"]);
        }
        None => heading = format!("Every app on {serial}:"),
    }
    let filter;
    if let Some(level) = level {
        filter = format!("*:{level}");
        args.push(&filter);
    }
    let out = adb(sdk, serial, &args, ADB_LIMIT)?;
    let log = out.stdout.trim();
    if log.is_empty() {
        return Ok(format!("{heading}\n(no lines)"));
    }
    Ok(format!("{heading}\n{log}"))
}

impl Server {
    /// The log of the device `serial`, kept to `variant` of the project's app when its id can be
    /// read from the project, streamed until the returned [`Logcat`] is dropped.
    ///
    /// # Errors
    ///
    /// Returns an error when the SDK has no adb, or adb does not start.
    pub fn android_logcat(
        &self,
        project: &MobileProject,
        serial: &str,
        variant: &Variant,
    ) -> Result<Logcat, Error> {
        let sdk = self.android_sdk(&project.root)?;
        let package = application_id(project, variant);
        let shared = Arc::new(Shared::default());
        match &package {
            Some(package) => {
                let (shared, serial, package) =
                    (Arc::clone(&shared), serial.to_string(), package.clone());
                std::thread::Builder::new()
                    .name(format!("logcat pid {serial}"))
                    .spawn(move || follow_app(&shared, &sdk, &serial, &package))?;
            }
            None => shared.follow(&sdk, serial, &["-T", DEVICE_BACKLOG])?,
        }
        Ok(Logcat { shared, package })
    }
}
