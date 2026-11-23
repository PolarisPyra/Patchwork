//! Timestamps and ids for what Patchwork records. Agent CLIs record times as RFC 3339 strings.

// Rust guideline compliant 2026-10-09

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Datelike, SecondsFormat, Utc};

/// The time now, in epoch milliseconds. 0 for a clock set before 1970.
#[must_use]
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Parses an RFC 3339 timestamp such as `2026-10-20T00:49:59.687+00:00` into epoch milliseconds.
/// None for a time before 1970.
#[must_use]
pub fn parse_rfc3339_ms(timestamp: &str) -> Option<u64> {
    let time = DateTime::parse_from_rfc3339(timestamp).ok()?;
    u64::try_from(time.timestamp_millis()).ok()
}

/// Formats epoch milliseconds as an RFC 3339 UTC timestamp such as `2026-10-20T00:49:59.687Z`.
/// None for a time past the year 9999, which RFC 3339 cannot write.
#[must_use]
pub fn format_rfc3339_ms(ms: u64) -> Option<String> {
    let time = DateTime::<Utc>::from_timestamp_millis(i64::try_from(ms).ok()?)?;
    (time.year() <= 9999).then(|| time.to_rfc3339_opts(SecondsFormat::AutoSi, true))
}

/// An id for a record Patchwork creates.
#[must_use]
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
