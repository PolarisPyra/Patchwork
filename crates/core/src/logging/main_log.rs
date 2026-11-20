//! `main.log`: the app's own `log` messages, and its panics.

// Rust guideline compliant 2026-10-09

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::Path;
use std::sync::Mutex;

use super::debug_log::app_logs;
use super::render::local_time;
use super::{redact, session_dir};
use crate::system::now_ms;

/// Writes the app's `log` messages and panics to this launch's `main.log`.
///
/// The file is in this launch's folder under `logs`, and takes messages at the level Settings
/// gives it. A panic is logged there before it is reported as usual. Nothing is written when
/// the file cannot be opened or another logger is already set.
pub fn start_main_log(logs: &Path) {
    let folder = session_dir(logs);
    let opened = std::fs::create_dir_all(&folder).and_then(|()| {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(folder.join("main.log"))
    });
    if let Ok(file) = opened
        && log::set_boxed_logger(Box::new(MainLog(Mutex::new(file)))).is_ok()
    {
        log::set_max_level(log::LevelFilter::Trace);
    }
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!(info:%; "panic: {{info}}");
        report(info);
    }));
}

/// The logger behind `main.log`.
struct MainLog(Mutex<File>);

impl log::Log for MainLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        app_logs(metadata.level())
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            log::Level::Warn => "warning".to_string(),
            level => level.as_str().to_lowercase(),
        };
        let time = local_time(now_ms());
        let mut properties = Properties(Vec::new());
        let _ = record.key_values().visit(&mut properties);
        let message = redact(&properties.fill(&record.args().to_string()));
        if let Ok(mut file) = self.0.lock() {
            let _ = writeln!(file, "{time} [{level}] {message}");
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.0.lock() {
            let _ = file.flush();
        }
    }
}

/// The named properties of a log record, written in place of their `{key}` in its message.
struct Properties(Vec<(String, String)>);

impl Properties {
    /// The message with each `{key}` it names replaced by that property's value.
    fn fill(&self, message: &str) -> String {
        let mut filled = String::with_capacity(message.len());
        let mut rest = message;
        while let Some(open) = rest.find('{') {
            filled.push_str(&rest[..open]);
            let named = rest[open + 1..].find('}').and_then(|close| {
                let key = &rest[open + 1..open + 1 + close];
                let value = self.0.iter().find(|(k, _)| k == key)?;
                Some((close, &value.1))
            });
            if let Some((close, value)) = named {
                filled.push_str(value);
                rest = &rest[open + close + 2..];
            } else {
                filled.push('{');
                rest = &rest[open + 1..];
            }
        }
        filled.push_str(rest);
        filled
    }
}

impl<'kvs> log::kv::VisitSource<'kvs> for Properties {
    fn visit_pair(
        &mut self,
        key: log::kv::Key<'kvs>,
        value: log::kv::Value<'kvs>,
    ) -> Result<(), log::kv::Error> {
        self.0.push((key.to_string(), value.to_string()));
        Ok(())
    }
}
