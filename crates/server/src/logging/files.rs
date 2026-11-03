//! The log files on disk: one folder per app session, rotated files, and reading and exporting them for Settings.

use std::fmt;
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use chrono::Local;
use ts_rs::TS;
use super::Json;
use super::scrub::scrubbed;

/// A log larger than this moves to `<name>.1.log`: VS Code's 30 MB over six files.
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// How many full logs are kept besides the current one, as `<name>.1.log` (the newest) on.
const KEEP: usize = 5;

/// How many launches' log folders are kept, this one included.
pub(super) const KEEP_SESSIONS: usize = 10;

/// The log files of versions before session folders, which could hold API keys.
const OLD_LOGS: [&str; 3] = ["agents.log", "imports.log", "app.log"];

/// How much of a log the Output panel shows when it opens it.
const MAX_READ_BYTES: u64 = 1024 * 1024;

/// This launch's log folder, made and tidied by the first call to `session_dir`.
static SESSION: OnceLock<PathBuf> = OnceLock::new();

/// This launch's folder, once `session_dir` has made it.
pub(super) fn session() -> Option<&'static Path> {
    SESSION.get().map(PathBuf::as_path)
}

/// This launch's folder in the logs folder `logs`. The first call makes it, deletes all but the
/// newest `KEEP_SESSIONS` folders, and deletes the single log files older versions wrote.
pub fn session_dir(logs: &Path) -> PathBuf {
    SESSION
        .get_or_init(|| {
            let root = logs.to_path_buf();
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
#[ts(export, export_to = "logging.ts")]
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

/// A log file, owned by the writer thread and opened at its first line. Failures to write
/// are ignored: the log is only for debugging.
pub(super) struct LogFile {
    pub(super) path: PathBuf,
    pub(super) out: Option<BufWriter<File>>,
    pub(super) written: u64,
    /// The size at which the log moves aside, and how many moved-aside files are kept.
    max_bytes: u64,
    keep: usize,
}

impl LogFile {
    pub(super) fn new(path: PathBuf) -> Self {
        Self::limited(path, MAX_BYTES, KEEP)
    }

    /// A log that moves aside at `max_bytes`, keeping `keep` older files.
    pub(super) fn limited(path: PathBuf, max_bytes: u64, keep: usize) -> Self {
        Self {
            path,
            out: None,
            written: 0,
            max_bytes,
            keep,
        }
    }

    pub(super) fn append(&mut self, line: &str) {
        if self.out.is_none() || self.written > self.max_bytes {
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
    pub(super) fn open(&mut self) {
        self.flush();
        self.out = None;
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let size = |p: &Path| std::fs::metadata(p).map_or(0, |m| m.len());
        if size(&self.path) > self.max_bytes {
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
    pub(super) fn rotate(&self) {
        let numbered = |n: usize| self.path.with_extension(format!("{n}.log"));
        let _ = std::fs::remove_file(numbered(self.keep));
        for n in (1..self.keep).rev() {
            let _ = std::fs::rename(numbered(n), numbered(n + 1));
        }
        let _ = std::fs::rename(&self.path, numbered(1));
    }

    pub(super) fn flush(&mut self) {
        if let Some(out) = self.out.as_mut() {
            let _ = out.flush();
        }
    }
}
