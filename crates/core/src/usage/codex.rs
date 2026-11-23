//! Codex subscription limits. Codex records its latest rate limits in each
//! session's rollout file (`token_count` events), so the newest one is used.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

use crate::provider;
use crate::system::time::parse_rfc3339_ms;
use crate::system::now_ms;
use crate::usage::{UsageWindow, ProviderUsage};

/// How many of the most recently modified rollout files are searched.
const RECENT_FILES: usize = 30;

/// Bytes read per step back from the end of a rollout; rollouts can be hundreds of MB.
const TAIL_CHUNK: u64 = 256 * 1024;

pub fn usage(mut out: ProviderUsage) -> Result<ProviderUsage, Error> {
    let sessions = provider::codex::home()
        .map(|h| h.join("sessions"))
        .ok_or("Codex home not found")?;
    let mut files = provider::codex::rollout_files(&sessions);
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let (at, limits) = files
        .iter()
        .take(RECENT_FILES)
        .find_map(|(_, path)| latest_limits(path))
        .ok_or("No usage reported yet — send a Codex message first")?;
    out.plan = limits["plan_type"].as_str().map(String::from);
    out.windows = ["primary", "secondary"]
        .iter()
        .filter_map(|key| window(&limits[*key], at))
        .collect();
    out.updated_at = Some(at);
    Ok(out)
}

/// The last `rate_limits` with a primary window in one rollout file, with its timestamp.
/// Reads backwards from the end in chunks, so a long rollout is never loaded whole.
fn latest_limits(path: &Path) -> Option<(u64, Value)> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut end = file.metadata().ok()?.len();
    // The unfinished line at the start of what has been read so far.
    let mut partial = Vec::new();
    while end > 0 {
        let start = end.saturating_sub(TAIL_CHUNK);
        let mut buf = vec![0; (end - start) as usize];
        file.seek(SeekFrom::Start(start)).ok()?;
        file.read_exact(&mut buf).ok()?;
        buf.extend_from_slice(&partial);
        let mut lines: Vec<&[u8]> = buf.split(|&b| b == b'\n').collect();
        if start > 0 {
            partial = lines.remove(0).to_vec();
        }
        if let Some(found) = lines.iter().rev().find_map(|line| parse_limits(line)) {
            return Some(found);
        }
        end = start;
    }
    None
}

fn parse_limits(line: &[u8]) -> Option<(u64, Value)> {
    let line = std::str::from_utf8(line).ok()?;
    if !line.contains("\"rate_limits\"") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let limits = &v["payload"]["rate_limits"];
    if !limits["primary"].is_object() {
        return None;
    }
    let at = v["timestamp"].as_str().and_then(parse_rfc3339_ms)?;
    Some((at, limits.clone()))
}

/// One window; a window whose reset time has passed is shown as unused.
fn window(w: &Value, reported_at: u64) -> Option<UsageWindow> {
    let used = w["used_percent"].as_f64()?;
    let minutes = w["window_minutes"].as_u64()?;
    let resets_at = w["resets_at"].as_u64().map(|s| s * 1000).or_else(|| {
        w["resets_in_seconds"]
            .as_u64()
            .map(|s| reported_at + s * 1000)
    });
    let expired = resets_at.is_some_and(|r| r <= now_ms());
    Some(UsageWindow {
        label: super::window_label(minutes),
        used_percent: if expired { 0.0 } else { used },
        resets_at: resets_at.filter(|_| !expired),
    })
}
