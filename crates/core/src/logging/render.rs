//! How one record reads in a log file: its time, level, source and text.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use chrono::{Local, TimeZone};
use crate::settings::LogLevel;
use crate::system::now_ms;
use super::scrub::{MAX_TEXT_CHARS, cut, redact, scrubbed};
use super::{Data, Json, Record, Source, may_report_error};

/// `2026-10-26 12:17:29.562 [info] [pid 1234] [thread a1b2c3d4] out: {…}`, with the data
/// made safe to share; None when a line checked at Error turns out not to report one.
pub(super) fn render(record: Record) -> Option<String> {
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
        let _ = write!(line, "[pid {pid}] ");
    }
    if let Some(thread) = thread {
        let _ = write!(line, "[thread {}] ", short(thread));
    }
    if let Some(message) = message {
        let _ = write!(line, "[message {}] ", short(message));
    }
    if !record.event.is_empty() {
        line.push_str(record.event);
        if !body.is_empty() {
            line.push_str(": ");
        }
    }
    // One record stays on one line.
    line.push_str(&body.replace('\n', "\\n"));
    Some(line)
}

/// The time now as log lines show it, for the desktop shell's `main.log`.
#[must_use]
pub fn now_text() -> String {
    local_time(now_ms())
}

/// `2026-10-26 12:17:29.562`, local time.
pub(super) fn local_time(ms: u64) -> String {
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

fn is_tool_result(fields: &[(String, Json)]) -> bool {
    fields.iter().any(|(key, value)| {
        matches!((key.as_str(), value), ("type", Json::String(s)) if s == "tool_result")
    })
}

/// Whether a line says something failed: a `type`, `subtype` or `status` naming an error or
/// a failure, or Claude's `is_error`.
fn reports_error(json: &Json) -> bool {
    match json {
        Json::Array(items) => items.iter().any(reports_error),
        // A command the agent ran that failed is its own result, not an error of the app.
        Json::Object(fields) if is_tool_result(fields) => false,
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
pub(super) fn outline(json: &mut Json, keep: bool) {
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
