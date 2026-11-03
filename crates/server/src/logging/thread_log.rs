//! Each chat's own log, `threads/<thread id>.log` in the launch's folder, beside its other logs:
//! what the agent was sent and did, each reply's end, and the agent's own traffic, at the level
//! set for Chats in Settings → Logging. A chat used over several launches has a log in each, and
//! its report joins them. A log moves to `<id>.1.log` past `MAX_BYTES`, and goes with its launch.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;

use crate::settings::LogLevel;
use crate::system::now_ms;

use super::files::{LogFile, THREADS_DIR, sessions};
use super::render::render;
use super::{Data, LaunchLog, Record, Source};

/// A chat's log moves aside past this, so a chat keeps at most twice as much.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Open chat logs past this many are closed, so a long session holds few files open.
const MAX_OPEN: usize = 32;

/// The chats' logs. A record about a chat in the agent log reaches here too, at this log's level.
pub static THREAD_LOG: ThreadLog = ThreadLog::new();

pub struct ThreadLog {
    writer: OnceLock<Sender<Record>>,
    dir: OnceLock<PathBuf>,
    level: AtomicU8,
}

impl LaunchLog for ThreadLog {
    fn init(&self, session: &Path) {
        let dir = session.join(THREADS_DIR);
        let (tx, rx) = mpsc::channel::<Record>();
        if self.writer.set(tx).is_err() {
            return;
        }
        let _ = self.dir.set(dir.clone());
        std::thread::spawn(move || {
            let mut files: HashMap<String, LogFile> = HashMap::new();
            while let Ok(record) = rx.recv() {
                write_record(&dir, &mut files, record);
                while let Ok(record) = rx.try_recv() {
                    write_record(&dir, &mut files, record);
                }
                files.values_mut().for_each(LogFile::flush);
                if files.len() > MAX_OPEN {
                    files.clear();
                }
            }
        });
    }

    fn set_level(&self, level: LogLevel) {
        self.level.store(level as u8, Ordering::Relaxed);
    }
}

impl ThreadLog {
    const fn new() -> Self {
        Self {
            writer: OnceLock::new(),
            dir: OnceLock::new(),
            level: AtomicU8::new(LogLevel::Off as u8),
        }
    }

    /// Whether a record at `level` is written.
    pub fn logs(&self, level: LogLevel) -> bool {
        level != LogLevel::Off && level as u8 <= self.level.load(Ordering::Relaxed)
    }

    /// Logs `data` at `level` to the log of `source`'s thread; nothing without one.
    pub fn write(&self, level: LogLevel, event: &'static str, source: &Source, data: Data) {
        if source.thread.is_none() || !self.logs(level) {
            return;
        }
        if let Some(tx) = self.writer.get() {
            let _ = tx.send(Record {
                at: now_ms(),
                level,
                file: "",
                event,
                source: source.clone(),
                data,
            });
        }
    }

    /// Logs a plain message about `thread`.
    pub fn text(&self, level: LogLevel, thread: &str, event: &'static str, text: String) {
        if !self.logs(level) {
            return;
        }
        let source = Source {
            thread: Some(thread.into()),
            ..Source::default()
        };
        self.write(level, event, &source, Data::Text(text));
    }

    /// The files of `thread`'s log in every kept launch, oldest first: in each, what moved aside,
    /// then the current one.
    pub fn files(&self, thread: &str) -> Vec<PathBuf> {
        let Some(root) = self.dir.get().and_then(|dir| dir.parent()?.parent()) else {
            return Vec::new();
        };
        sessions(root)
            .iter()
            .map(|session| session.join(THREADS_DIR))
            .flat_map(|dir| {
                [
                    log_path(&dir, thread).with_extension("1.log"),
                    log_path(&dir, thread),
                ]
            })
            .filter(|path| path.is_file())
            .collect()
    }
}

fn log_path(dir: &Path, thread: &str) -> PathBuf {
    dir.join(format!("{thread}.log"))
}

fn write_record(dir: &Path, files: &mut HashMap<String, LogFile>, record: Record) {
    let Some(thread) = record.source.thread.clone() else {
        return;
    };
    // Ids are made by Patchwork, but a name that could leave the folder is never written.
    if thread.is_empty() || thread.contains(['/', '\\', '.']) {
        return;
    }
    if let Some(line) = render(record) {
        files
            .entry(thread.clone())
            .or_insert_with(|| LogFile::limited(log_path(dir, &thread), MAX_BYTES, 1))
            .append(&line);
    }
}
