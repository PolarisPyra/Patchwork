//! Each chat's own log, `chat-<thread id>.log` in the launch's folder, beside its other logs:
//! what the agent was sent and did, each reply's end, and the agent's own traffic, at the level
//! set for Chats in Settings → General → Logs.
//!
//! Only a chat with a problem gets its file. Its lines are held in memory, the newest
//! `HELD_BYTES`, until it logs a warning or an error, and from then on are written as they come.
//! Its Open logs writes the file too, with the chat's report at the top (see `report`), so one
//! file per chat holds all there is to send. A file past `MAX_BYTES` drops its oldest lines, and
//! a chat used over several launches has a file in each.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;
use std::time::Duration;

use crate::settings::LogLevel;
use crate::system::fs::home_dir;
use crate::system::now_ms;

use super::files::{CHAT_PREFIX, sessions};
use super::render::render;
use super::{Data, LaunchLog, Record, Source};

/// A chat's file past this drops its oldest log lines, down to half.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// How much is held of a chat without a problem: its newest lines.
const HELD_BYTES: usize = 512 * 1024;

/// Open chat files past this many are closed, so a long session holds few files open.
const MAX_OPEN: usize = 32;

/// How long Open logs waits for the writer.
const REPORT_WAIT: Duration = Duration::from_secs(5);

/// The line in a chat's file between its report and its log.
pub(super) const LOG_START: &str = "== Chat log ==";

/// The chats' logs. A record about a chat in the agent log reaches here too, at this log's level.
pub static THREAD_LOG: ThreadLog = ThreadLog::new();

#[derive(Debug)]
pub struct ThreadLog {
    writer: OnceLock<Sender<Message>>,
    dir: OnceLock<PathBuf>,
    level: AtomicU8,
}

enum Message {
    Record(Record),
    /// Writes the chat's file with `report` at the top, and answers with its path.
    Report {
        thread: String,
        report: String,
        done: Sender<Result<PathBuf, Error>>,
    },
}

impl LaunchLog for ThreadLog {
    fn init(&self, session: &Path) {
        let (tx, rx) = mpsc::channel::<Message>();
        if self.writer.set(tx).is_err() {
            return;
        }
        let _ = self.dir.set(session.to_path_buf());
        let mut writer = Writer {
            dir: session.to_path_buf(),
            home: home_dir()
                .map(|home| home.display().to_string())
                .filter(|home| home.len() > 1),
            chats: HashMap::new(),
        };
        std::thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                writer.handle(message);
                while let Ok(message) = rx.try_recv() {
                    writer.handle(message);
                }
                writer.flush();
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
            let _ = tx.send(Message::Record(Record {
                at: now_ms(),
                level,
                file: "",
                event,
                source: source.clone(),
                data,
            }));
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

    /// Writes `thread`'s file in this launch's folder with `report` at the top, followed by
    /// `LOG_START` and the chat's log, and returns its path.
    pub(super) fn report(&self, thread: &str, report: String) -> Result<PathBuf, Error> {
        let tx = self.writer.get().ok_or("There are no logs yet")?;
        let (done, answer) = mpsc::channel();
        tx.send(Message::Report {
            thread: thread.into(),
            report,
            done,
        })
        .map_err(|_discarded| "The chat log has stopped".to_string())?;
        answer
            .recv_timeout(REPORT_WAIT)
            .map_err(|_discarded| "The chat log did not answer".to_string())?
    }

    /// `thread`'s files in the kept launches before this one, oldest first.
    pub(super) fn earlier(&self, thread: &str) -> Vec<PathBuf> {
        let Some(dir) = self.dir.get() else {
            return Vec::new();
        };
        let Some(root) = dir.parent() else {
            return Vec::new();
        };
        sessions(root)
            .iter()
            .filter(|session| *session != dir)
            .map(|session| log_path(session, thread))
            .filter(|path| path.is_file())
            .collect()
    }
}

/// The log in a chat's file: what follows its report, or all of it without one.
pub(super) fn log_part(text: &str) -> &str {
    split(text).1
}

/// A chat's file as its report up to and with the `LOG_START` line, if it has one, and its log.
fn split(text: &str) -> (Option<&str>, &str) {
    let marker = format!("\n{LOG_START}\n");
    match text.find(&marker) {
        Some(i) => {
            let (head, log) = text.split_at(i + marker.len());
            (Some(head), log)
        }
        None => (None, text),
    }
}

fn log_path(dir: &Path, thread: &str) -> PathBuf {
    dir.join(format!("{CHAT_PREFIX}{thread}.log"))
}

/// Ids are made by Patchwork, but a name that could leave the folder is never written.
fn valid(thread: &str) -> bool {
    !thread.is_empty() && !thread.contains(['/', '\\', '.'])
}

/// The writer thread's chats, owned by it alone.
struct Writer {
    dir: PathBuf,
    /// The home folder, written as `~` so a chat's file names no user.
    home: Option<String>,
    chats: HashMap<String, Chat>,
}

impl Writer {
    fn handle(&mut self, message: Message) {
        match message {
            Message::Record(record) => self.record(record),
            Message::Report {
                thread,
                report,
                done,
            } => {
                let _ = done.send(self.report(&thread, &report));
            }
        }
    }

    fn record(&mut self, record: Record) {
        let Some(thread) = record.source.thread.clone() else {
            return;
        };
        if !valid(&thread) {
            return;
        }
        let problem = matches!(record.level, LogLevel::Error | LogLevel::Warning);
        let Some(mut line) = render(record) else {
            return;
        };
        if let Some(home) = &self.home {
            line = line.replace(home.as_str(), "~");
        }
        let path = log_path(&self.dir, &thread);
        let chat = self.chats.entry(thread).or_default();
        chat.push(line);
        if problem {
            chat.keep(&path);
        }
    }

    fn report(&mut self, thread: &str, report: &str) -> Result<PathBuf, Error> {
        if !valid(thread) {
            return Err(format!("Not a chat: {thread}").into());
        }
        let path = log_path(&self.dir, thread);
        self.chats
            .entry(thread.to_string())
            .or_default()
            .keep(&path)
            .rewrite(Some(report), MAX_BYTES)
            .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        Ok(path)
    }

    /// Writes out what is buffered, and closes the files if too many are open.
    fn flush(&mut self) {
        let files: Vec<&mut ChatFile> = self
            .chats
            .values_mut()
            .filter_map(|chat| chat.file.as_mut())
            .collect();
        // Only the open ones: a chat's file stays in the list after it is closed.
        let close = files.iter().filter(|file| file.out.is_some()).count() > MAX_OPEN;
        for file in files {
            if close {
                file.close();
            } else {
                file.flush();
            }
        }
    }
}

/// A chat as the writer has it: its newest lines while it has had no problem, then its file.
#[derive(Default)]
struct Chat {
    held: VecDeque<String>,
    held_bytes: usize,
    file: Option<ChatFile>,
}

impl Chat {
    fn push(&mut self, line: String) {
        if let Some(file) = &mut self.file {
            file.append(&line);
            return;
        }
        self.held_bytes += line.len() + 1;
        self.held.push_back(line);
        while self.held_bytes > HELD_BYTES {
            let Some(old) = self.held.pop_front() else {
                break;
            };
            self.held_bytes -= old.len() + 1;
        }
    }

    /// The chat's file, which from now on takes its lines, starting with those held so far.
    fn keep(&mut self, path: &Path) -> &mut ChatFile {
        let file = self.file.get_or_insert_with(|| ChatFile::new(path));
        for line in self.held.drain(..) {
            file.append(&line);
        }
        self.held_bytes = 0;
        file
    }
}

/// A chat's file. Failures to append are ignored: the log is only for debugging.
struct ChatFile {
    path: PathBuf,
    out: Option<BufWriter<File>>,
    size: u64,
}

impl ChatFile {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            out: None,
            size: 0,
        }
    }

    fn append(&mut self, line: &str) {
        if self.out.is_none() {
            self.open();
        }
        if self.size > MAX_BYTES {
            let _ = self.rewrite(None, MAX_BYTES / 2);
        }
        let Some(out) = self.out.as_mut() else { return };
        if writeln!(out, "{line}").is_ok() {
            self.size += line.len() as u64 + 1;
        }
    }

    fn open(&mut self) {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path);
        self.out = file.ok().map(BufWriter::new);
        self.size = std::fs::metadata(&self.path).map_or(0, |m| m.len());
    }

    fn flush(&mut self) {
        if let Some(out) = self.out.as_mut() {
            let _ = out.flush();
        }
    }

    fn close(&mut self) {
        self.flush();
        self.out = None;
    }

    /// Writes the file again as `report` (or the report it has) and the `LOG_START` line, then
    /// the newest lines of its log that fit in `keep` bytes.
    fn rewrite(&mut self, report: Option<&str>, keep: u64) -> std::io::Result<()> {
        self.close();
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        let (head, log) = split(&text);
        let head = match report {
            Some(report) => format!("{}\n{LOG_START}\n", report.trim_end()),
            None => head.unwrap_or_default().to_string(),
        };
        let cut = log
            .len()
            .saturating_sub(usize::try_from(keep).unwrap_or(usize::MAX));
        let log = match cut {
            0 => log,
            // From the first line that starts after the cut.
            _ => log.as_bytes()[cut..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or("", |i| &log[cut + i + 1..]),
        };
        std::fs::write(&self.path, format!("{head}{log}"))?;
        self.open();
        Ok(())
    }
}
