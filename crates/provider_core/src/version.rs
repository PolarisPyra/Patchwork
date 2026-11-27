//! Semantic version comparison helpers for installed agent CLIs.

// Rust guideline compliant 2026-10-09

/// `1.5.16` as `[1, 5, 16]`, ignoring a `v` before it and anything after `-`, `+`, or whitespace.
#[must_use]
pub fn version_parts(version: &str) -> Option<Vec<u32>> {
    let number = version.trim().trim_start_matches('v');
    let number = number.split(['-', '+', ' ']).next()?;
    number.split('.').map(|part| part.parse().ok()).collect()
}

/// Whether `version` is older than `than`. Versions that cannot be read are never older.
#[must_use]
pub fn is_older(version: &str, than: &str) -> bool {
    match (version_parts(version), version_parts(than)) {
        (Some(have), Some(newest)) => have < newest,
        _ => false,
    }
}
