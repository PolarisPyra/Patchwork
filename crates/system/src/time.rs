//! Timestamps and ids for what Patchwork records. Agent CLIs record times as RFC 3339 strings.

// Rust guideline compliant 2026-10-09

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const NANOS_PER_MS: i128 = 1_000_000;

/// The time now, in epoch milliseconds.
#[must_use]
pub fn now_ms() -> u64 {
    epoch_ms(OffsetDateTime::now_utc()).unwrap_or(0)
}

/// `time` as epoch milliseconds. None for a time before 1970.
fn epoch_ms(time: OffsetDateTime) -> Option<u64> {
    u64::try_from(time.unix_timestamp_nanos() / NANOS_PER_MS).ok()
}

/// Parses an RFC 3339 timestamp such as `2026-10-20T00:49:59.687+00:00` into epoch milliseconds.
#[must_use]
pub fn parse_rfc3339_ms(timestamp: &str) -> Option<u64> {
    OffsetDateTime::parse(timestamp, &Rfc3339)
        .ok()
        .and_then(epoch_ms)
}

/// Formats epoch milliseconds as an RFC 3339 UTC timestamp such as `2026-10-20T00:49:59.687Z`.
/// None for a time past the year 9999, which RFC 3339 cannot write.
#[must_use]
pub fn format_rfc3339_ms(ms: u64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * NANOS_PER_MS)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

/// An id for a record Patchwork creates.
#[must_use]
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utc_offsets_and_fractions() {
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_ms("2026-10-19T16:14:21.270Z"),
            Some(1_792_426_461_270)
        );
        assert_eq!(
            parse_rfc3339_ms("2026-10-19T18:14:21.270123+02:00"),
            parse_rfc3339_ms("2026-10-19T16:14:21.270Z")
        );
        assert_eq!(parse_rfc3339_ms("nope"), None);
    }
}
