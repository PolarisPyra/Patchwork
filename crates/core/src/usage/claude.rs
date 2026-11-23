//! Claude subscription limits from the claude.ai OAuth usage endpoint, using
//! the token Claude Code stored when the user signed in.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::provider::claude::credentials;
use crate::system::time::parse_rfc3339_ms;
use crate::system::now_ms;
use crate::usage::{UsageWindow, ProviderUsage};
use super::{Failure, RATE_LIMIT_BACKOFF};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
/// The plan changes rarely, so the profile is asked for at most this often.
const PLAN_FRESH_FOR: Duration = Duration::from_secs(6 * 60 * 60);

/// When the live plan was last read, and what it said (None if the read failed).
static LIVE_PLAN: Mutex<Option<(Instant, Option<String>)>> = Mutex::new(None);

/// Windows the endpoint reports, in display order.
const WINDOWS: [(&str, &str); 4] = [
    ("five_hour", "Session (5h)"),
    ("seven_day", "Weekly"),
    ("seven_day_opus", "Weekly · Opus"),
    ("seven_day_sonnet", "Weekly · Sonnet"),
];

pub(super) fn usage(mut out: ProviderUsage) -> Result<ProviderUsage, Failure> {
    let creds = credentials().ok_or("Not signed in with a Claude subscription")?;
    let oauth = &creds["claudeAiOauth"];
    let token = oauth["accessToken"]
        .as_str()
        .ok_or("Not signed in with a Claude subscription")?;
    if oauth["expiresAt"]
        .as_u64()
        .is_some_and(|exp| exp < now_ms())
    {
        return Err("Sign-in expired — run `claude` to refresh it".into());
    }
    out.plan = oauth["subscriptionType"].as_str().map(String::from);

    let agent: ureq::Agent = ureq::Agent::config_builder()
        // The usage panel can wait this long; past it, the usage is shown as unknown.
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .get(USAGE_URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .call()
        .map_err(|e| format!("Couldn't load usage: {e}"))?;
    match response.status().as_u16() {
        200..=299 => {}
        429 => {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse().ok())
                .map_or(RATE_LIMIT_BACKOFF, Duration::from_secs);
            let minutes = retry_after.as_secs().div_ceil(60).max(1);
            return Err(Failure {
                message: format!(
                    "Anthropic is rate-limiting usage checks; trying again in about {minutes} min"
                ),
                retry_after: Some(retry_after),
            });
        }
        code => return Err(format!("Couldn't load usage (HTTP {code})").into()),
    }
    let body: Value = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("Couldn't load usage: {e}"))?;
    out.windows = windows(&body);
    out.updated_at = Some(now_ms());
    // The stored plan is only set at sign-in, so it misses upgrades; prefer the live one.
    if let Some(plan) = live_plan_for(&agent, token) {
        out.plan = Some(plan);
    }
    Ok(out)
}

/// The live plan, asked for at most once per `PLAN_FRESH_FOR`, failures included,
/// so a rate-limited profile request is not repeated on every usage refresh.
fn live_plan_for(agent: &ureq::Agent, token: &str) -> Option<String> {
    let mut cache = LIVE_PLAN.locked();
    if let Some((at, plan)) = cache.as_ref()
        && at.elapsed() < PLAN_FRESH_FOR
    {
        return plan.clone();
    }
    let plan = agent
        .get(PROFILE_URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .call()
        .ok()
        .filter(|r| r.status().is_success())
        .and_then(|mut r| r.body_mut().read_json::<Value>().ok())
        .and_then(|profile| live_plan(&profile));
    *cache = Some((Instant::now(), plan.clone()));
    plan
}

/// e.g. "max 5x" from organization type "`claude_max`" and tier "`default_claude_max_5x`".
fn live_plan(profile: &Value) -> Option<String> {
    let org = &profile["organization"];
    let plan = org["organization_type"]
        .as_str()?
        .trim_start_matches("claude_");
    let multiplier = org["rate_limit_tier"]
        .as_str()
        .and_then(|tier| tier.rsplit('_').next())
        .filter(|m| m.ends_with('x') && m[..m.len() - 1].parse::<u32>().is_ok());
    Some(match multiplier {
        Some(m) => format!("{plan} {m}"),
        None => plan.to_string(),
    })
}

/// The plan Claude Code stored at sign-in.
pub fn stored_plan() -> Option<String> {
    credentials()?["claudeAiOauth"]["subscriptionType"]
        .as_str()
        .map(String::from)
}

/// The windows in a `rate_limit_event` Claude reports during a turn, which carry the
/// same numbers as the usage endpoint (as fractions, with resets in seconds).
#[must_use]
pub fn windows_from_event(info: &Value) -> Vec<UsageWindow> {
    let reported = &info["unifiedWindows"];
    WINDOWS
        .iter()
        .filter_map(|(key, label)| {
            let w = &reported[*key];
            Some(UsageWindow {
                label: (*label).into(),
                used_percent: w["utilization"].as_f64()? * 100.0,
                resets_at: w["resetsAt"].as_u64().map(|s| s * 1000),
            })
        })
        .collect()
}

fn windows(body: &Value) -> Vec<UsageWindow> {
    let mut out: Vec<UsageWindow> = WINDOWS
        .iter()
        .filter_map(|(key, label)| {
            let w = &body[*key];
            Some(UsageWindow {
                label: (*label).into(),
                used_percent: w["utilization"].as_f64()?,
                resets_at: w["resets_at"].as_str().and_then(parse_rfc3339_ms),
            })
        })
        .collect();
    // Newer models, such as Fable, have no key of their own: their weekly limit is a
    // `weekly_scoped` entry in `limits`, named by the model it covers.
    for limit in body["limits"].as_array().into_iter().flatten() {
        if limit["kind"] != "weekly_scoped" {
            continue;
        }
        let (Some(model), Some(used_percent)) = (
            limit["scope"]["model"]["display_name"].as_str(),
            limit["percent"].as_f64(),
        ) else {
            continue;
        };
        let label = format!("Weekly · {model}");
        if out.iter().any(|w| w.label == label) {
            continue;
        }
        out.push(UsageWindow {
            label,
            used_percent,
            resets_at: limit["resets_at"].as_str().and_then(parse_rfc3339_ms),
        });
    }
    out
}
