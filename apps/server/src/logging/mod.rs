//! Debug logs, laid out and written the way VS Code writes its own, for people to send with a
//! bug report. Each launch writes a folder, `<config>/logs/<YYYYMMDDTHHMMSS>` in local time,
//! and the newest `KEEP_SESSIONS` are kept:
//!
//! ```text
//! 20261026T121729/
//! ├─ main.log                 Patchwork's own messages (written by the desktop shell)
//! ├─ imports.log              scans and imports of saved conversations
//! ├─ window/renderer.log     the window's errors and warnings
//! └─ agents/claude.log        one log per agent: its processes and their traffic
//! ```
//!
//! Each line reads `2026-10-26 12:17:29.562 [info] message`, and each log is set to Trace,
//! Debug, Info, Warning, Error or Off in Settings → Logging, or from the Output panel. A full
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

mod files;
mod render;
mod scrub;

pub use files::{session_dir, channels, LogChunk, read, export};
use files::LogFile;
pub use render::{now_text};
use render::render;
pub use scrub::{Json};
use scrub::{MAX_TEXT_CHARS, scrubbed, redact, cut};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;

use crate::provider::AgentKind;
use crate::settings::LogLevel;
use crate::system::now_ms;

/// The agent logs, `agents/<agent>.log`.
pub static AGENTS: DebugLog = DebugLog::new("agents/agents.log");

/// How much `main.log` records. The desktop shell's logger checks it for every message, since
/// the logger sets the `log` crate's own maximum when it starts.
static APP_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);

/// Follows the setting for `main.log`.
pub fn set_app_level(level: LogLevel) {
    APP_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Whether `main.log` records a message at `level`.
pub fn app_logs(level: log::Level) -> bool {
    let level = match level {
        log::Level::Error => LogLevel::Error,
        log::Level::Warn => LogLevel::Warning,
        log::Level::Info => LogLevel::Info,
        log::Level::Debug => LogLevel::Debug,
        log::Level::Trace => LogLevel::Trace,
    };
    level as u8 <= APP_LEVEL.load(Ordering::Relaxed)
}

/// The window's errors and warnings, `window/renderer.log`.
pub static RENDERER: DebugLog = DebugLog::new("window/renderer.log");

pub struct DebugLog {
    writer: OnceLock<Sender<Record>>,
    level: AtomicU8,
    /// The log's file in the launch's folder, for records from no agent.
    file: &'static str,
}

/// What a record carries, made into text on the writer thread.
pub enum Data {
    /// A line an agent sent or printed, in full.
    Line(String),
    /// A line an agent sent or printed, as an outline unless it reports an error.
    Outline(String),
    Text(String),
}

struct Record {
    at: u64,
    level: LogLevel,
    file: &'static str,
    event: &'static str,
    source: Source,
    data: Data,
}

/// Where a record comes from: the agent, its process, and the thread and reply it works on.
#[derive(Clone, Default)]
pub struct Source {
    pub agent: Option<AgentKind>,
    pub pid: Option<u32>,
    pub thread: Option<String>,
    pub message: Option<String>,
}

impl Source {
    pub fn agent(agent: AgentKind, pid: u32, thread: &str, message: Option<&str>) -> Self {
        Self {
            agent: Some(agent),
            pid: Some(pid),
            thread: Some(thread.into()),
            message: message.map(String::from),
        }
    }
}

/// Each agent's log in the launch's folder.
fn agent_file(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "agents/claude.log",
        AgentKind::Codex => "agents/codex.log",
        AgentKind::Antigravity => "agents/antigravity.log",
        AgentKind::OpenCode => "agents/opencode.log",
        AgentKind::Api => "agents/api.log",
    }
}

impl DebugLog {
    pub const fn new(file: &'static str) -> Self {
        Self {
            writer: OnceLock::new(),
            level: AtomicU8::new(LogLevel::Off as u8),
            file,
        }
    }

    /// Starts the writer thread for the log in the launch's folder `dir`. Called once when
    /// the server opens.
    pub fn init(&self, dir: PathBuf) {
        let (tx, rx) = mpsc::channel::<Record>();
        if self.writer.set(tx).is_err() {
            return;
        }
        std::thread::spawn(move || {
            let mut files: HashMap<&'static str, LogFile> = HashMap::new();
            // Waits for a record, writes it and every one queued behind it, then flushes.
            while let Ok(record) = rx.recv() {
                write_record(&dir, &mut files, record);
                while let Ok(record) = rx.try_recv() {
                    write_record(&dir, &mut files, record);
                }
                files.values_mut().for_each(LogFile::flush);
            }
        });
    }

    /// Follows the setting.
    pub fn set_level(&self, level: LogLevel) {
        self.level.store(level as u8, Ordering::Relaxed);
    }

    /// Whether a record at `level` is written.
    pub fn logs(&self, level: LogLevel) -> bool {
        level != LogLevel::Off && level as u8 <= self.level.load(Ordering::Relaxed)
    }

    /// Logs `data` at `level`. `event` names what happened, or is empty for a plain message.
    pub fn write(&self, level: LogLevel, event: &'static str, source: &Source, data: Data) {
        if !self.logs(level) {
            return;
        }
        if let Some(tx) = self.writer.get() {
            let _ = tx.send(Record {
                at: now_ms(),
                level,
                file: source.agent.map_or(self.file, agent_file),
                event,
                source: source.clone(),
                data,
            });
        }
    }

    /// Logs a line an agent sent or printed: in full at Trace, as an outline at Debug, and
    /// below that only when it may report an error. Copies the line only when it is logged.
    pub fn line(&self, event: &'static str, source: &Source, line: &str) {
        if self.logs(LogLevel::Trace) {
            self.write(LogLevel::Trace, event, source, Data::Line(line.into()));
        } else if self.logs(LogLevel::Debug) {
            self.write(LogLevel::Debug, event, source, Data::Outline(line.into()));
        } else if self.logs(LogLevel::Error) && may_report_error(line) {
            self.write(LogLevel::Error, event, source, Data::Outline(line.into()));
        }
    }
}

/// A quick check before a line is sent to be logged at Error; the writer checks it properly.
fn may_report_error(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("error") || line.contains("fail")
}

fn write_record(dir: &Path, files: &mut HashMap<&'static str, LogFile>, record: Record) {
    let file = record.file;
    if let Some(line) = render(record) {
        files
            .entry(file)
            .or_insert_with(|| LogFile::new(dir.join(file)))
            .append(&line);
    }
}
