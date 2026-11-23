//! Errors agy reports outside JSON-RPC errors: usage-limit notices it writes as reply text, and
//! failed model calls it records only in its session log while ending the reply normally.

// Rust guideline compliant 2026-11-23

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::provider::Parsed;
use crate::system::now_ms;
use crate::usage::UsageWindow;

use super::client::home;

/// The step type agy's session log records a failed model call as.
const ERROR_STEP: i64 = 17;

/// How much of an error step is read: the error leads it, and a stack trace follows.
const ERROR_BYTES: i64 = 16_384;

/// How long reading the session log waits while agy, which writes it, holds it.
const LOG_BUSY: Duration = Duration::from_millis(100);

/// The usage window a reached limit is noted under, so the app can offer to resume once it
/// resets.
const LIMIT_WINDOW: &str = "Quota reached";

/// What agy says when the model's quota for the period is used up.
pub(super) const QUOTA_MESSAGE: &str = "You have reached your Antigravity quota for this period. Wait for \
    it to reset, or pick another model.";

/// Google's API error as agy writes it in its own words: HTTP status, message, and status
/// name.
static API_ERROR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)Error (\d{3}), Message: (.+?), Status: ([A-Z_]+)")
        .expect("API_ERROR is a valid pattern")
});

/// Google's API error as agy's session log holds it, such as `NOT_FOUND (code 404): Requested
/// entity was not found.`: status name, HTTP status, and message up to the next field.
static LOGGED_API_ERROR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([A-Z][A-Z_]+) \(code (\d{3})\): ([^\x00-\x1f]+)")
        .expect("LOGGED_API_ERROR is a valid pattern")
});

/// A Go duration, such as `1h12m40s` or `3.5s`, after a word that says when a limit resets.
static RESET_DELAY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:resets? in|quotaResetDelay:|retryDelay:)\s*((?:\d+(?:\.\d+)?(?:ns|us|µs|ms|h|m|s))+)",
    )
        .expect("RESET_DELAY is a valid pattern")
});

/// One part of a Go duration: an amount and its unit.
static DURATION_PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d+(?:\.\d+)?)(ns|us|µs|ms|h|m|s)").expect("DURATION_PART is a valid pattern")
});

/// The usage-limit error agy writes into the reply as text, such as `⚠ Individual quota
/// reached. … Resets in 1h12m40s.`, before ending it as a normal reply.
pub(super) fn limit_notice(text: &str) -> Option<&str> {
    let notice = text
        .trim()
        .strip_prefix('⚠')?
        .trim_start_matches('\u{fe0f}')
        .trim();
    let lower = notice.to_lowercase();
    [
        "quota",
        "resource_exhausted",
        "429",
        "exhausted your capacity",
        "rate limit",
    ]
    .iter()
    .any(|term| lower.contains(term))
    .then_some(notice)
}

/// A used-up usage window resetting when `text` says the limit resets, so the reply can be
/// resumed then. None when `text` gives no time.
pub(super) fn limit_window(text: &str) -> Option<Parsed> {
    let delay = RESET_DELAY
        .captures(text)
        .and_then(|c| go_duration_ms(&c[1]))?;
    Some(Parsed::Usage(vec![UsageWindow {
        label: LIMIT_WINDOW.into(),
        used_percent: 100.0,
        resets_at: Some(now_ms().saturating_add(delay)),
    }]))
}

/// Milliseconds in a Go duration such as `1h12m40s`. None when it holds no time.
fn go_duration_ms(text: &str) -> Option<u64> {
    let ms: f64 = DURATION_PART
        .captures_iter(text)
        .map(|c| {
            let amount: f64 = c[1].parse().unwrap_or(0.0);
            let unit = match &c[2] {
                "h" => 3_600_000.0,
                "m" => 60_000.0,
                "s" => 1_000.0,
                "ms" => 1.0,
                "us" | "µs" => 1e-3,
                _ => 1e-6,
            };
            amount * unit
        })
        .sum();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive delay of hours fits in u64 milliseconds"
    )]
    (ms >= 1.0).then_some(ms.round() as u64)
}

/// What a reply agy ended normally failed on, read from the last step of its session log:
/// agy writes a failed model call there and ends the reply as if it went well. Empty when the
/// last step is no error or the log cannot be read.
pub(super) fn logged_error(session_id: &str) -> Vec<Parsed> {
    let Some(text) = last_error_step(session_id) else {
        return Vec::new();
    };
    let api = api_error(&text);
    let limited = text.contains("QUOTA_EXHAUSTED")
        || api
            .as_ref()
            .is_some_and(|e| e.code == "429" || e.status == "RESOURCE_EXHAUSTED");
    let message = if text.contains("QUOTA_EXHAUSTED") {
        QUOTA_MESSAGE.to_string()
    } else if let Some(api) = &api {
        format!(
            "Antigravity's model call failed: {} ({})",
            api.message, api.status
        )
    } else if text.contains("blocked by content safety filters") {
        "A response was blocked by the model's content safety filters. Rephrase the request, or \
         try another model."
            .into()
    } else {
        "Antigravity stopped this reply because of an error. Send a message to try again.".into()
    };
    let mut out = vec![Parsed::Error(message)];
    if limited {
        out.extend(limit_window(&text));
    }
    out
}

/// A Google API error found in an error step.
#[derive(Debug)]
struct ApiError<'a> {
    code: &'a str,
    message: &'a str,
    status: &'a str,
}

/// The first Google API error in `text`, in either form agy writes one.
fn api_error(text: &str) -> Option<ApiError<'_>> {
    if let Some(c) = LOGGED_API_ERROR.captures(text) {
        let (status, code, message) = (c.get(1)?, c.get(2)?, c.get(3)?);
        return Some(ApiError {
            code: code.as_str(),
            message: message.as_str().trim(),
            status: status.as_str(),
        });
    }
    let c = API_ERROR.captures(text)?;
    let (code, message, status) = (c.get(1)?, c.get(2)?, c.get(3)?);
    Some(ApiError {
        code: code.as_str(),
        message: message.as_str().trim(),
        status: status.as_str(),
    })
}

/// The text of the last step in the session's log when that step is an error.
fn last_error_step(session_id: &str) -> Option<String> {
    let path = log_path(session_id)?;
    let read = || -> rusqlite::Result<Option<String>> {
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(LOG_BUSY)?;
        let step: Option<(i64, Vec<u8>)> = conn
            .query_row(
                "SELECT step_type, CAST(substr(step_payload, 1, ?1) AS BLOB) FROM steps \
                 ORDER BY idx DESC LIMIT 1",
                [ERROR_BYTES],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(step
            .filter(|(kind, _)| *kind == ERROR_STEP)
            .map(|(_, payload)| String::from_utf8_lossy(&payload).into_owned()))
    };
    read().unwrap_or_else(|e| {
        log::warn!(error:% = e; "could not read the Antigravity session log: {{error}}");
        None
    })
}

/// Where agy keeps the session's log. None for an id that is not a plain name.
fn log_path(session_id: &str) -> Option<PathBuf> {
    let plain = !session_id.is_empty()
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let path = home()?
        .join("antigravity-cli")
        .join("conversations")
        .join(format!("{session_id}.db"));
    (plain && path.is_file()).then_some(path)
}
