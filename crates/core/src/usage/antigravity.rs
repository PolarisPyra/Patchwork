//! Antigravity's quota, from its own `/usage` command, read the way its own dashboard reads it.
//!
//! agy splits the quota by model family and gives each family a weekly and a 5-hour window. It
//! reports what is *left* of each window, as a fraction and as a whole percent, and it names the
//! window by that: "Weekly Limit Remaining", "Five Hour Limit Remaining".

// Rust guideline compliant 2026-11-24

use crate::Error;
use serde_json::Value;

use crate::provider::probe_until;
use crate::system::time::parse_rfc3339_ms;
use crate::system::now_ms;
use crate::provider::AgentKind;
use crate::usage::{ProviderUsage, UsageWindow};
use super::types::FIVE_HOUR_LABEL;

pub fn usage(mut out: ProviderUsage) -> Result<ProviderUsage, Error> {
    // The CLI prints one JSON object and then keeps running, so stop once that object is complete.
    let p = probe_until(
        AgentKind::Antigravity,
        &["-p", "/usage", "--output-format", "json"],
        |stdout| serde_json::from_str::<Value>(stdout.trim()).is_ok(),
    )
    .ok_or("Antigravity's usage did not answer in time")?;
    let v: Value = serde_json::from_str(p.stdout.trim()).map_err(|_discarded| {
        let message = p.stderr.lines().last().unwrap_or("Usage could not be read");
        message.to_string()
    })?;
    if v["status"] != "SUCCESS" {
        return Err(v["error"]
            .as_str()
            .unwrap_or("Usage could not be read")
            .into());
    }
    let groups = v["command"]["data"]["groups"]
        .as_array()
        .into_iter()
        .flatten();
    out.windows.extend(groups.flat_map(|group| {
        let family = group["name"]
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(String::from);
        group["buckets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(move |bucket| {
                let left = bucket["remaining_fraction"]
                    .as_f64()
                    .map(|left| left.clamp(0.0, 1.0))?;
                // agy shows what is left to the whole percent, so the app shows the same number
                // it does, and what is used is the rest of that.
                let left_percent = (left * 100.0).round();
                Some(UsageWindow {
                    label: window_label(bucket["window"].as_str().unwrap_or_default()),
                    group: family.clone(),
                    used_percent: 100.0 - left_percent,
                    // A window nothing has used has no reset to wait for; agy names one anyway.
                    resets_at: (left_percent < 100.0)
                        .then(|| bucket["reset_time"].as_str().and_then(parse_rfc3339_ms))
                        .flatten(),
                })
            })
    }));
    out.plan = crate::provider::antigravity::tier();
    out.updated_at = Some(now_ms());
    Ok(out)
}

/// What agy calls a window, by the token it sends: "weekly" and "5h".
fn window_label(window: &str) -> String {
    match window.trim() {
        "weekly" => "Weekly".into(),
        "5h" => FIVE_HOUR_LABEL.into(),
        "" => "Limit".into(),
        other => other.into(),
    }
}
