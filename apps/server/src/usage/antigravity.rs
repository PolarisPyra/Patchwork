//! Antigravity's quota, from its own `/usage` command.

use serde_json::Value;

use crate::provider::probe_until;
use crate::time::parse_rfc3339_ms;
use crate::types::{AgentKind, UsageWindow, ProviderUsage};

pub fn usage(mut out: ProviderUsage) -> Result<ProviderUsage, String> {
    // The CLI prints one JSON object and then keeps running, so stop once that object is complete.
    let p = probe_until(
        AgentKind::Antigravity,
        &["-p", "/usage", "--output-format", "json"],
        |stdout| serde_json::from_str::<Value>(stdout.trim()).is_ok(),
    )
    .ok_or("Antigravity CLI is not installed")?;
    let v: Value = serde_json::from_str(p.stdout.trim()).map_err(|_| {
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
                UsageWindow {
                    label: format!(
                        "{} · {}",
                        group["name"].as_str().unwrap_or_default(),
                        bucket["window"].as_str().unwrap_or_default()
                    ),
                    used_percent: ((1.0 - remaining) * 100.0).clamp(0.0, 100.0),
                    resets_at: bucket["reset_time"].as_str().and_then(parse_rfc3339_ms),
                }
            })
    }));
    Ok(out)
}
