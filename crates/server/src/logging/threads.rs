//! Each chat's own log, `logs/threads/<thread id>.log`. Unlike the launch folders, it is kept
//! across launches, so one file tells a chat's whole story: what the agent was sent and did, each
//! reply's end, and the agent's own traffic, at the level set for Chats in Settings → Logging.
//! A log moves to `<id>.1.log` past `MAX_BYTES`, and the log of a chat that no longer exists is
//! deleted at the next launch.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;

use crate::settings::LogLevel;
use crate::system::now_ms;

use super::files::LogFile;
use super::render::render;
use super::{Data, Record, Source};

/// A chat's log moves aside past this, so a chat keeps at most twice as much.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Open chat logs past this many are closed, so a long session holds few files open.
const MAX_OPEN: usize = 32;

/// The chats' logs.
pub static THREADS: ThreadLogs = ThreadLogs::new();

pub struct ThreadLogs {
    writer: OnceLock<Sender<Record>>,
    dir: OnceLock<PathBuf>,
    level: AtomicU8,
}

impl ThreadLogs {
    const fn new() -> Self {
        Self {
            writer: OnceLock::new(),
            dir: OnceLock::new(),
            level: AtomicU8::new(LogLevel::Off as u8),
        }
    }

    /// Starts the writer thread for the logs in `dir`, `logs/threads`. Called once when the
    /// server opens.
    pub fn init(&self, dir: PathBuf) {
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

    /// Follows the setting.
    pub fn set_level(&self, level: LogLevel) {
        self.level.store(level as u8, Ordering::Relaxed);
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

    /// The files of `thread`'s log, oldest first: what moved aside, then the current one.
    pub fn files(&self, thread: &str) -> Vec<PathBuf> {
        let Some(dir) = self.dir.get() else {
            return Vec::new();
        };
        [
            log_path(dir, thread).with_extension("1.log"),
            log_path(dir, thread),
        ]
        .into_iter()
        .filter(|path| path.is_file())
        .collect()
    }

    /// Deletes the logs of chats not in `threads`, which were deleted while the app was closed
    /// or before it kept chat logs.
    pub fn prune(&self, threads: &HashSet<String>) {
        let Some(dir) = self.dir.get() else { return };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let id = name.split('.').next().unwrap_or_default();
            if name.ends_with(".log") && !threads.contains(id) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
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
