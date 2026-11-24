//! Subscription usage limits per provider.

// Rust guideline compliant 2026-10-09

use serde::Serialize;

use crate::provider::AgentKind;

/// What Claude and Codex call their 5-hour window.
pub(super) const SESSION_LABEL: &str = "Session (5h)";
/// What agy calls its 5-hour window.
pub(super) const FIVE_HOUR_LABEL: &str = "5-hour";

/// One rate-limit window, e.g. the 5-hour session limit.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    /// What the window itself is called, e.g. "Weekly" or "Session (5h)".
    pub label: String,
    /// The models the window covers, for a provider that splits its limits by model family.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// 0–100.
    pub used_percent: f64,
    /// Unix epoch milliseconds.
    pub resets_at: Option<u64>,
}

impl UsageWindow {
    /// What is left of the window, 0–100, as the provider itself reports it.
    #[must_use]
    pub fn remaining_percent(&self) -> f64 {
        (100.0 - self.used_percent).clamp(0.0, 100.0)
    }

    /// Whether this is a provider's 5-hour window.
    #[must_use]
    pub fn is_five_hour(&self) -> bool {
        self.label == SESSION_LABEL || self.label == FIVE_HOUR_LABEL
    }
}

/// Subscription usage limits for one agent, as reported by its provider.
#[derive(Debug, Clone, Serialize)]
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
