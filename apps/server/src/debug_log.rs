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

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{LazyLock, OnceLock};

use chrono::{Local, TimeZone};
use regex::Regex;
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use ts_rs::TS;

use crate::types::{AgentKind, LogLevel, now_ms};

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

/// A log larger than this moves to `<name>.1.log`: VS Code's 30 MB over six files.
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// How many full logs are kept besides the current one, as `<name>.1.log` (the newest) on.
const KEEP: usize = 5;

/// Strings in logged JSON longer than this are cut to it.
const MAX_STRING_CHARS: usize = 300;

/// Text that is not JSON is cut to this.
const MAX_TEXT_CHARS: usize = 4000;

/// How many launches' log folders are kept, this one included.
const KEEP_SESSIONS: usize = 10;

/// The log files of versions before session folders, which could hold API keys.
const OLD_LOGS: [&str; 3] = ["agents.log", "imports.log", "app.log"];

/// How much of a log the Output panel shows when it opens it.
const MAX_READ_BYTES: u64 = 1024 * 1024;

/// This launch's log folder, made and tidied by the first call to `session_dir`.
static SESSION: OnceLock<PathBuf> = OnceLock::new();

/// This launch's folder in `<config>/logs`. The first call makes it, deletes all but the
/// newest `KEEP_SESSIONS` folders, and deletes the single log files older versions wrote.
pub fn session_dir(config_dir: &Path) -> PathBuf {
    SESSION
        .get_or_init(|| {
            let root = config_dir.join("logs");
            let dir = root.join(Local::now().format("%Y%m%dT%H%M%S").to_string());
            let _ = std::fs::create_dir_all(&dir);
            let entries: Vec<_> = std::fs::read_dir(&root)
                .map(|list| list.flatten().collect())
                .unwrap_or_default();
            let mut sessions: Vec<PathBuf> = entries
                .iter()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && is_session(&e.file_name()))
                .map(|e| e.path())
                .collect();
            sessions.sort();
            let old = sessions.len().saturating_sub(KEEP_SESSIONS);
            for session in &sessions[..old] {
                let _ = std::fs::remove_dir_all(session);
            }
            for entry in &entries {
                let name = entry.file_name().to_string_lossy().into_owned();
                if OLD_LOGS.iter().any(|log| name.starts_with(log)) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
            dir
        })
        .clone()
}

/// `YYYYMMDDTHHMMSS`.
fn is_session(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    name.len() == 15
        && name
            .char_indices()
            .all(|(i, c)| if i == 8 { c == 'T' } else { c.is_ascii_digit() })
}

/// Every file in `dir` and its subfolders, sorted.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map(|list| list.flatten().collect())
        .unwrap_or_default();
    entries.sort_by_key(|e| e.file_name());
    let mut files = Vec::new();
    for entry in entries {
        match entry.file_type() {
            Ok(t) if t.is_dir() => files.extend(files_under(&entry.path())),
            Ok(t) if t.is_file() => files.push(entry.path()),
            _ => {}
        }
    }
    files
}

/// This launch's logs, as paths from its folder, for the Output panel. Older files a log has
/// moved aside (`<name>.1.log`) are left out.
pub fn channels() -> Vec<String> {
    let Some(dir) = SESSION.get() else {
        return Vec::new();
    };
    files_under(dir)
        .iter()
        .filter_map(|path| path.strip_prefix(dir).ok())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .filter(|name| {
            let stem = name.trim_end_matches(".log");
            name.ends_with(".log")
                && !stem
                    .rsplit('.')
                    .next()
                    .is_some_and(|n| n.parse::<u32>().is_ok())
        })
        .collect()
}

/// What the Output panel reads of a log.
#[derive(serde::Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LogChunk {
    pub text: String,
    /// Where to read from next.
    pub offset: u64,
    /// The log started over (it filled up and moved aside), so the panel clears it first.
    pub reset: bool,
}

/// The log `channel` (a path from `channels`) from `offset` on. A first read, or a read after
/// the log started over, shows at most its last `MAX_READ_BYTES`, from a line start.
pub fn read(channel: &str, offset: u64) -> Result<LogChunk, String> {
    let dir = SESSION.get().ok_or("There are no logs yet")?;
    let path = Path::new(channel);
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(format!("Not a log: {channel}"));
    }
    let mut file =
        File::open(dir.join(path)).map_err(|e| format!("Could not open {channel}: {e}"))?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    let reset = offset > size;
    let mut from = if reset { 0 } else { offset };
    let trimmed = (from == 0 && size > MAX_READ_BYTES).then(|| size - MAX_READ_BYTES);
    if let Some(start) = trimmed {
        from = start;
    }
    file.seek(SeekFrom::Start(from))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(size - from)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    // A cut start drops its partial first line; the end stops after the last full line.
    let start = match trimmed {
        Some(_) => bytes
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |i| i + 1),
        None => 0,
    };
    let end = bytes
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(start, |i| (i + 1).max(start));
    Ok(LogChunk {
        text: String::from_utf8_lossy(&bytes[start..end]).into_owned(),
        offset: from + end as u64,
        reset: reset || trimmed.is_some(),
    })
}

/// Writes a zip at `dest` of every kept launch's log folder, plus `system.json`: what a
/// friend sends with a bug report. Records still queued are not waited for.
pub fn export(dest: &Path, system: &serde_json::Value) -> Result<(), String> {
    let Some(root) = SESSION.get().and_then(|dir| dir.parent()) else {
        return Err("There are no logs yet".into());
    };
    let file =
        File::create(dest).map_err(|e| format!("Could not create {}: {e}", dest.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    let failed = |e: &dyn fmt::Display| format!("Could not write the logs: {e}");
    let system = serde_json::from_value::<Json>(system.clone()).map_err(|e| failed(&e))?;
    let system = serde_json::to_string_pretty(&scrubbed(system)).map_err(|e| failed(&e))?;
    zip.start_file("system.json", options)
        .map_err(|e| failed(&e))?;
    zip.write_all(system.as_bytes()).map_err(|e| failed(&e))?;
    let mut sessions: Vec<PathBuf> = std::fs::read_dir(root)
        .map_err(|e| failed(&e))?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && is_session(&e.file_name()))
        .map(|e| e.path())
        .collect();
    sessions.sort();
    for session in sessions {
        for path in files_under(&session) {
            let Ok(name) = path.strip_prefix(root) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let name = name.to_string_lossy().replace('\\', "/");
            zip.start_file(name, options).map_err(|e| failed(&e))?;
            zip.write_all(&bytes).map_err(|e| failed(&e))?;
        }
    }
    zip.finish().map_err(|e| failed(&e))?;
    Ok(())
}

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

/// A log file, owned by the writer thread and opened at its first line. Failures to write
/// are ignored: the log is only for debugging.
struct LogFile {
    path: PathBuf,
    out: Option<BufWriter<File>>,
    written: u64,
}

impl LogFile {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            out: None,
            written: 0,
        }
    }

    fn append(&mut self, line: &str) {
        if self.out.is_none() || self.written > MAX_BYTES {
            self.open();
        }
        let Some(out) = self.out.as_mut() else { return };
        if out
            .write_all(line.as_bytes())
            .and_then(|()| out.write_all(b"\n"))
            .is_ok()
        {
            self.written += line.len() as u64 + 1;
        }
    }

    /// Opens the log for appending, first moving a full one aside.
    fn open(&mut self) {
        self.flush();
        self.out = None;
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let size = |p: &Path| std::fs::metadata(p).map_or(0, |m| m.len());
        if size(&self.path) > MAX_BYTES {
            self.rotate();
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path);
        self.out = file.ok().map(BufWriter::new);
        self.written = size(&self.path);
    }

    /// `<name>.1.log` becomes `<name>.2.log` and so on, dropping the oldest; the log becomes
    /// `<name>.1.log`.
    fn rotate(&self) {
        let numbered = |n: usize| self.path.with_extension(format!("{n}.log"));
        let _ = std::fs::remove_file(numbered(KEEP));
        for n in (1..KEEP).rev() {
            let _ = std::fs::rename(numbered(n), numbered(n + 1));
        }
        let _ = std::fs::rename(&self.path, numbered(1));
    }

    fn flush(&mut self) {
        if let Some(out) = self.out.as_mut() {
            let _ = out.flush();
        }
    }
}

/// `2026-10-26 12:17:29.562 [info] [pid 1234] [thread a1b2c3d4] out: {…}`, with the data
/// made safe to share; None when a line checked at Error turns out not to report one.
fn render(record: Record) -> Option<String> {
    let mut level = record.level;
    let body = match record.data {
        Data::Line(line) => match serde_json::from_str::<Json>(&line) {
            Ok(json @ (Json::Object(_) | Json::Array(_))) => to_text(&scrubbed(json)),
            _ => cut(&redact(&line), MAX_TEXT_CHARS),
        },
        Data::Outline(line) => match serde_json::from_str::<Json>(&line) {
            Ok(json @ (Json::Object(_) | Json::Array(_))) if reports_error(&json) => {
                level = LogLevel::Error;
                to_text(&scrubbed(json))
            }
            // Below Debug, only a line that reports an error is logged.
            _ if level == LogLevel::Error => return None,
            Ok(mut json @ (Json::Object(_) | Json::Array(_))) => {
                outline(&mut json, false);
                to_text(&scrubbed(json))
            }
            _ => format!("<{} chars>", line.chars().count()),
        },
        Data::Text(text) => cut(&redact(&text), MAX_TEXT_CHARS),
    };
    let level = match level {
        LogLevel::Off => "off",
        LogLevel::Error => "error",
        LogLevel::Warning => "warning",
        LogLevel::Info => "info",
        LogLevel::Debug => "debug",
        LogLevel::Trace => "trace",
    };
    let short = |id: String| id.chars().take(8).collect::<String>();
    let Source {
        pid,
        thread,
        message,
        ..
    } = record.source;
    let mut line = format!("{} [{level}] ", local_time(record.at));
    if let Some(pid) = pid {
        line.push_str(&format!("[pid {pid}] "));
    }
    if let Some(thread) = thread {
        line.push_str(&format!("[thread {}] ", short(thread)));
    }
    if let Some(message) = message {
        line.push_str(&format!("[message {}] ", short(message)));
    }
    if !record.event.is_empty() {
        line.push_str(record.event);
        line.push_str(": ");
    }
    // One record stays on one line.
    line.push_str(&body.replace('\n', "\\n"));
    Some(line)
}

/// The time now as log lines show it, for the desktop shell's `main.log`.
pub fn now_text() -> String {
    local_time(now_ms())
}

/// `2026-10-26 12:17:29.562`, local time.
fn local_time(ms: u64) -> String {
    Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .unwrap_or_else(Local::now)
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string()
}

fn to_text(json: &Json) -> String {
    serde_json::to_string(json).unwrap_or_default()
}

/// Whether a line says something failed: a `type`, `subtype` or `status` naming an error or
/// a failure, or Claude's `is_error`.
fn reports_error(json: &Json) -> bool {
    match json {
        Json::Array(items) => items.iter().any(reports_error),
        Json::Object(fields) => fields
            .iter()
            .any(|(key, value)| match (key.as_str(), value) {
                ("type" | "subtype" | "status", Json::String(s)) => may_report_error(s),
                ("is_error", Json::Bool(b)) => *b,
                (_, value) => reports_error(value),
            }),
        _ => false,
    }
}

/// Replaces each string with its length, except the ones that say what a line is: its type,
/// names, statuses and ids.
fn outline(json: &mut Json, keep: bool) {
    match json {
        Json::String(s) if !keep => *s = format!("<{} chars>", s.chars().count()),
        Json::Array(items) => items.iter_mut().for_each(|item| outline(item, keep)),
        Json::Object(fields) => {
            for (key, value) in fields.iter_mut() {
                outline(value, names_the_line(key));
            }
        }
        _ => {}
    }
}

fn names_the_line(key: &str) -> bool {
    const KEYS: [&str; 13] = [
        "type",
        "subtype",
        "kind",
        "key",
        "name",
        "tool",
        "status",
        "event",
        "role",
        "method",
        "action",
        "model",
        "stop_reason",
    ];
    KEYS.contains(&key) || key.ends_with("id") || key.ends_with("ID") || key.ends_with("Id")
}

fn scrubbed(mut json: Json) -> Json {
    scrub(&mut json, false);
    json
}

/// Hides string values under secret-looking keys, and every value under `headers`; redacts
/// and cuts the other strings.
fn scrub(json: &mut Json, secret: bool) {
    match json {
        Json::String(s) if secret => *s = "[REDACTED]".into(),
        Json::String(s) => *s = cut(&redact(s), MAX_STRING_CHARS),
        Json::Array(items) => items.iter_mut().for_each(|item| scrub(item, secret)),
        Json::Object(fields) => {
            for (key, value) in fields.iter_mut() {
                scrub(value, secret || is_secret(key));
            }
        }
        _ => {}
    }
}

/// `apiKey`, `x-api-key`, `access_token`, `password`, `headers` and the like.
fn is_secret(key: &str) -> bool {
    let key: String = key.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let key = key.to_ascii_lowercase();
    [
        "apikey",
        "token",
        "secret",
        "password",
        "authorization",
        "cookie",
        "headers",
        "credential",
    ]
    .iter()
    .any(|word| key.ends_with(word))
}

/// What a secret in free text looks like, and what it is replaced with.
static SECRETS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        // A PEM private key, to its end or the end of the text.
        (
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?(?:-----END [A-Z ]*PRIVATE KEY-----|$)",
            "[REDACTED PRIVATE KEY]",
        ),
        // `NAME=value`, `NAME: value` and `"name": "value"` where the name ends in a secret word.
        (
            r#"(?i)\b([A-Z0-9_.-]*(?:api[_-]?key|secret|token|password|passwd|pwd|access[_-]?key|private[_-]?key|credentials?))("?\s*[:=]\s*["']?)[^\s"',;]{4,}"#,
            "${1}${2}[REDACTED]",
        ),
        // `Authorization: Bearer …` and the like.
        (r"(?i)\b(bearer|basic|token)\s+[A-Za-z0-9._~+/=-]{16,}", "${1} [REDACTED]"),
        // `scheme://user:password@host`.
        (r"\b([a-z][a-z0-9+.-]*://)[^\s:/@]+:[^\s@/]+@", "${1}[REDACTED]@"),
        // Keys and tokens with a known shape.
        (
            concat!(
                r"\b(?:sk-ant-[A-Za-z0-9_-]{16,}|sk-(?:proj-)?[A-Za-z0-9_-]{20,}",
                r"|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|glpat-[A-Za-z0-9_-]{20,}",
                r"|xox[abposr]-[A-Za-z0-9-]{10,}|(?:AKIA|ASIA)[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{35}",
                r"|hf_[A-Za-z0-9]{30,}|npm_[A-Za-z0-9]{36}|(?:sk|rk)_live_[0-9A-Za-z]{20,}",
                r"|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})"
            ),
            "[REDACTED]",
        ),
    ]
    .into_iter()
    .filter_map(|(pattern, with)| Regex::new(pattern).ok().map(|re| (re, with)))
    .collect()
});

fn redact(text: &str) -> String {
    SECRETS.iter().fold(text.to_string(), |text, (re, with)| {
        re.replace_all(&text, *with).into_owned()
    })
}

/// The first `max` characters, and how many more there were.
fn cut(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((end, _)) => format!("{}…(+{} chars)", &s[..end], s[end..].chars().count()),
        None => s.to_string(),
    }
}

/// JSON that keeps an object's fields in the order they were written.
pub enum Json {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Serialize for Json {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Json::Null => serializer.serialize_unit(),
            Json::Bool(b) => serializer.serialize_bool(*b),
            Json::Number(n) => n.serialize(serializer),
            Json::String(s) => serializer.serialize_str(s),
            Json::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Json::Object(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (key, value) in fields {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonVisitor)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("JSON")
    }
    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }
    fn visit_bool<E>(self, b: bool) -> Result<Json, E> {
        Ok(Json::Bool(b))
    }
    fn visit_i64<E>(self, n: i64) -> Result<Json, E> {
        Ok(Json::Number(n.into()))
    }
    fn visit_u64<E>(self, n: u64) -> Result<Json, E> {
        Ok(Json::Number(n.into()))
    }
    fn visit_f64<E>(self, n: f64) -> Result<Json, E> {
        Ok(serde_json::Number::from_f64(n).map_or(Json::Null, Json::Number))
    }
    fn visit_str<E>(self, s: &str) -> Result<Json, E> {
        Ok(Json::String(s.into()))
    }
    fn visit_string<E>(self, s: String) -> Result<Json, E> {
        Ok(Json::String(s))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Json::Array(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut fields = Vec::new();
        while let Some(field) = map.next_entry()? {
            fields.push(field);
        }
        Ok(Json::Object(fields))
    }
}
