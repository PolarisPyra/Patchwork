//! Antigravity's quota, from its own `/usage` command.

// Rust guideline compliant 2026-10-09

use crate::Error;
use serde_json::Value;

use crate::provider::probe_until;
use crate::system::time::parse_rfc3339_ms;
use crate::system::now_ms;
use crate::provider::AgentKind;
use crate::usage::{UsageWindow, ProviderUsage};

pub fn usage(mut out: ProviderUsage) -> Result<ProviderUsage, Error> {
    // The CLI prints one JSON object and then keeps running, so stop once that object is complete.
    let p = probe_until(
        AgentKind::Antigravity,
        &["-p", "/usage", "--output-format", "json"],
        |stdout| serde_json::from_str::<Value>(stdout.trim()).is_ok(),
    )
    .ok_or("Antigravity CLI is not installed")?;
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
        group["buckets"]
            .as_array()
            .into_iter()
            .flatten()
            .map(move |bucket| {
                let remaining = bucket["remaining_fraction"].as_f64().unwrap_or(1.0);
                // Agy rounds what remains to a whole percent, so what is used is the rest of
                // that, and the two always add up to 100.
                let remaining_percent = (remaining * 100.0).round().clamp(0.0, 100.0);
                UsageWindow {
                    label: format!(
                        "{} · {}",
                        group["name"].as_str().unwrap_or_default(),
                        bucket["window"].as_str().unwrap_or_default()
                    ),
                    used_percent: 100.0 - remaining_percent,
                    // A window nothing has used yet has no reset to wait for; agy names none.
                    resets_at: (remaining < 1.0)
                        .then(|| bucket["reset_time"].as_str().and_then(parse_rfc3339_ms))
                        .flatten(),
                }
            })
    }));
    out.plan = crate::provider::antigravity::tier();
    out.updated_at = Some(now_ms());
    Ok(out)
}
