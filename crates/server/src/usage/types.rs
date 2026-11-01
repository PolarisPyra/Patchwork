//! Subscription usage limits per provider.

use serde::Serialize;
use ts_rs::TS;

use crate::provider::AgentKind;

/// One rate-limit window, e.g. the 5-hour session limit.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "usage.ts")]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub label: String,
    /// 0–100.
    pub used_percent: f64,
    /// Unix epoch milliseconds.
    pub resets_at: Option<u64>,
}

/// Subscription usage limits for one agent, as reported by its provider.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "usage.ts")]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsage {
    pub kind: AgentKind,
    /// e.g. "max", "plus".
    pub plan: Option<String>,
    pub windows: Vec<UsageWindow>,
    /// Why no windows are shown, e.g. "Not signed in".
    pub error: Option<String>,
    /// When the provider last reported these numbers (epoch ms).
    pub updated_at: Option<u64>,
    /// While the provider is rate-limiting usage checks, when it allows the next one (epoch ms).
    pub retry_at: Option<u64>,
}
