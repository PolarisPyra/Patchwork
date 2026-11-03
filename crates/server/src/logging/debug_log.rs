//! The debug log: what is recorded, for which agent, and writing it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;

use crate::provider::AgentKind;
use crate::settings::LogLevel;
use crate::system::now_ms;

use super::files::LogFile;
use super::render::render;
use super::{LaunchLog, THREAD_LOG};

/// The agent logs, `agent-<agent>.log`, with `agents.log` for records from no agent. A record
/// about a chat goes to its thread log too.
pub static AGENT_LOG: DebugLog = DebugLog::new("agents.log");

/// The window's errors and warnings, `window.log`.
pub static WINDOW_LOG: DebugLog = DebugLog::new("window.log");

/// `imports.log`: where each scan looked, what it found, what it skipped and why, and what an
/// import added. Written with `import::ilog!`.
pub static IMPORT_LOG: DebugLog = DebugLog::new("imports.log");

/// How much `main.log` records. The desktop shell's logger checks it for every message, since
/// the logger sets the `log` crate's own maximum when it starts.
static APP_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);

/// Follows the setting for `main.log`.
pub(super) fn set_app_level(level: LogLevel) {
    APP_LEVEL.store(level as u8, Ordering::Relaxed);
    log::set_max_level(match level {
        LogLevel::Off => log::LevelFilter::Off,
        LogLevel::Error => log::LevelFilter::Error,
        LogLevel::Warning => log::LevelFilter::Warn,
        LogLevel::Info => log::LevelFilter::Info,
        LogLevel::Debug => log::LevelFilter::Debug,
        LogLevel::Trace => log::LevelFilter::Trace,
    });
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

pub struct DebugLog {
    writer: OnceLock<Sender<Record>>,
    level: AtomicU8,
    /// The log's file in the launch's folder, for records from no agent.
    file: &'static str,
}

/// What a record carries, made into text on the writer thread.
#[derive(Clone)]
pub enum Data {
    /// A line an agent sent or printed, in full.
    Line(String),
    /// A line an agent sent or printed, as an outline unless it reports an error.
    Outline(String),
    Text(String),
}

pub(super) struct Record {
    pub(super) at: u64,
    pub(super) level: LogLevel,
    pub(super) file: &'static str,
    pub(super) event: &'static str,
    pub(super) source: Source,
    pub(super) data: Data,
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
        AgentKind::Claude => "agent-claude.log",
        AgentKind::Codex => "agent-codex.log",
        AgentKind::Antigravity => "agent-antigravity.log",
        AgentKind::OpenCode => "agent-opencode.log",
        AgentKind::Api => "agent-api.log",
    }
}

impl LaunchLog for DebugLog {
    fn init(&self, session: &Path) {
        let (tx, rx) = mpsc::channel::<Record>();
        if self.writer.set(tx).is_err() {
            return;
        }
        let dir = session.to_path_buf();
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

    fn set_level(&self, level: LogLevel) {
        self.level.store(level as u8, Ordering::Relaxed);
    }
}

impl DebugLog {
    const fn new(file: &'static str) -> Self {
        Self {
            writer: OnceLock::new(),
            level: AtomicU8::new(LogLevel::Off as u8),
            file,
        }
    }

    /// Whether a record at `level` is written.
    pub fn logs(&self, level: LogLevel) -> bool {
        level != LogLevel::Off && level as u8 <= self.level.load(Ordering::Relaxed)
    }

    /// Logs `data` at `level`, and to the log of the chat it is about, if any, at that log's own
    /// level. `event` names what happened, or is empty for a plain message.
    pub fn write(&self, level: LogLevel, event: &'static str, source: &Source, data: Data) {
        if source.thread.is_some() && THREAD_LOG.logs(level) {
            THREAD_LOG.write(level, event, source, data.clone());
        }
        self.send(level, event, source, data);
    }

    /// Logs `data` at `level` to this log alone.
    fn send(&self, level: LogLevel, event: &'static str, source: &Source, data: Data) {
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
    /// below that only when it may report an error. The chat's log takes it the same way at its
    /// own level. Copies the line only when it is logged.
    pub fn line(&self, event: &'static str, source: &Source, line: &str) {
        if let Some((level, data)) = line_record(|level| self.logs(level), line) {
            self.send(level, event, source, data);
        }
        if source.thread.is_some()
            && let Some((level, data)) = line_record(|level| THREAD_LOG.logs(level), line)
        {
            THREAD_LOG.write(level, event, source, data);
        }
    }
}

/// How a log at the levels `logs` accepts records an agent's line, if at all.
fn line_record(logs: impl Fn(LogLevel) -> bool, line: &str) -> Option<(LogLevel, Data)> {
    if logs(LogLevel::Trace) {
        Some((LogLevel::Trace, Data::Line(line.into())))
    } else if logs(LogLevel::Debug) {
        Some((LogLevel::Debug, Data::Outline(line.into())))
    } else if logs(LogLevel::Error) && may_report_error(line) {
        Some((LogLevel::Error, Data::Outline(line.into())))
    } else {
        None
    }
}

/// A quick check before a line is sent to be logged at Error; the writer checks it properly.
pub(super) fn may_report_error(line: &str) -> bool {
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
