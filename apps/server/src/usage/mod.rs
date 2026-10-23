//! Subscription usage limits (session / weekly windows) per agent.
//!
//! Claude's numbers come from a rate-limited web endpoint, so results are
//! cached: reopening the usage popup reuses them, refreshing is throttled, and
//! a 429 backs off while the last good numbers stay visible.

pub mod claude;
mod codex;
mod antigravity;

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::types::{AgentKind, UsageWindow, ProviderUsage, now_ms};

/// How long fetched usage is reused before it is fetched again.
const FRESH_FOR: Duration = Duration::from_secs(5 * 60);
/// Minimum time between fetches, even for an explicit refresh.
const MIN_REFRESH: Duration = Duration::from_secs(60);
/// Wait after a 429 that did not say how long to wait.
const RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(5 * 60);

/// Why usage couldn't be read.
#[derive(Debug)]
struct Failure {
    message: String,
    /// Set when the provider asked us to slow down.
    retry_after: Option<Duration>,
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self {
            message,
            retry_after: None,
        }
    }
}

fn empty(kind: AgentKind) -> ProviderUsage {
    ProviderUsage {
        kind,
        plan: None,
        windows: vec![],
        error: None,
        updated_at: None,
    }
}

fn fetch(kind: AgentKind) -> Result<ProviderUsage, Failure> {
    match kind {
        AgentKind::Claude => claude::usage(empty(kind)),
        AgentKind::Codex => codex::usage(empty(kind)).map_err(Failure::from),
        AgentKind::Antigravity => antigravity::usage(empty(kind)).map_err(Failure::from),
    }
}

struct Entry {
    fetched: Instant,
    /// The numbers came from the agent itself during a turn, not from a fetch.
    reported: bool,
    usage: ProviderUsage,
    /// No requests before this, after a rate limit.
    blocked_until: Option<Instant>,
}

#[derive(Default)]
pub struct UsageCache {
    entries: Mutex<HashMap<AgentKind, Entry>>,
}

impl UsageCache {
    /// Usage for `kind`. `refresh` asks for new numbers, still subject to throttling.
    pub fn get(&self, kind: AgentKind, refresh: bool) -> ProviderUsage {
        self.get_with(kind, refresh, Instant::now(), || fetch(kind))
    }

    fn get_with(
        &self,
        kind: AgentKind,
        refresh: bool,
        now: Instant,
        fetch: impl FnOnce() -> Result<ProviderUsage, Failure>,
    ) -> ProviderUsage {
        let lock = || self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(e) = lock().get(&kind) {
            let age = now.saturating_duration_since(e.fetched);
            let blocked = e.blocked_until.is_some_and(|until| now < until);
            // Only an explicit refresh fetches; opening the popup shows the last numbers.
            // Numbers the agent reported are as current as a fetch, so they are not
            // fetched again, even on refresh, while fresh.
            let fresh = age < FRESH_FOR && e.reported;
            if !refresh || blocked || age < MIN_REFRESH || fresh {
                return e.usage.clone();
            }
        }
        let result = fetch();
        let mut entries = lock();
        let entry = match result {
            Ok(usage) => Entry {
                fetched: now,
                reported: false,
                usage,
                blocked_until: None,
            },
            Err(failure) => {
                // Keep the last good numbers, with the reason they are not current.
                let mut usage = entries
                    .get(&kind)
                    .map(|e| e.usage.clone())
                    .filter(|u| !u.windows.is_empty())
                    .unwrap_or_else(|| empty(kind));
                usage.error = Some(failure.message);
                Entry {
                    fetched: now,
                    reported: false,
                    usage,
                    blocked_until: failure.retry_after.map(|d| now + d.max(MIN_REFRESH)),
                }
            }
        };
        let usage = entry.usage.clone();
        entries.insert(kind, entry);
        usage
    }
}

impl UsageCache {
    /// Records the windows the agent reported during a turn, so the popup shows them
    /// without asking the rate-limited endpoint. Windows it did not report are kept.
    pub fn record(&self, kind: AgentKind, windows: Vec<UsageWindow>) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let previous = entries.remove(&kind);
        let mut usage = previous.as_ref().map_or_else(
            || ProviderUsage {
                plan: claude::stored_plan(),
                ..empty(kind)
            },
            |e| e.usage.clone(),
        );
        for window in windows {
            match usage.windows.iter_mut().find(|w| w.label == window.label) {
                Some(w) => *w = window,
                None => usage.windows.push(window),
            }
        }
        usage.error = None;
        usage.updated_at = Some(now_ms());
        entries.insert(
            kind,
            Entry {
                fetched: Instant::now(),
                reported: true,
                usage,
                blocked_until: previous.and_then(|e| e.blocked_until),
            },
        );
    }
}

/// "Session (5h)" for a 300-minute window, "Weekly" for 10080, and so on.
fn window_label(minutes: u64) -> String {
    match minutes {
        300 => "Session (5h)".into(),
        10_080 => "Weekly".into(),
        m if m % 1440 == 0 => format!("{}-day", m / 1440),
        m => format!("{}h", (m as f64 / 60.0).round()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage_with(percent: f64) -> ProviderUsage {
        ProviderUsage {
            windows: vec![UsageWindow {
                label: "Weekly".into(),
                used_percent: percent,
                resets_at: None,
            }],
            ..empty(AgentKind::Claude)
        }
    }

    #[test]
    fn reuses_fresh_numbers_and_throttles_refresh() {
        let cache = UsageCache::default();
        let t0 = Instant::now();
        let first = cache.get_with(AgentKind::Claude, false, t0, || Ok(usage_with(10.0)));
        assert_eq!(first.windows[0].used_percent, 10.0);

        // Reopening never fetches, and refreshing within a minute does not either.
        let soon = t0 + Duration::from_secs(30);
        let again = cache.get_with(AgentKind::Claude, true, soon, || panic!("fetched"));
        assert_eq!(again.windows[0].used_percent, 10.0);
        let later = t0 + Duration::from_secs(120);
        cache.get_with(AgentKind::Claude, false, later, || panic!("fetched"));
        cache.get_with(AgentKind::Claude, false, t0 + FRESH_FOR, || {
            panic!("fetched")
        });

        // An explicit refresh after a minute does fetch.
        let refreshed = cache.get_with(AgentKind::Claude, true, later, || Ok(usage_with(20.0)));
        assert_eq!(refreshed.windows[0].used_percent, 20.0);
    }

    #[test]
    fn rate_limit_keeps_last_numbers_and_backs_off() {
        let cache = UsageCache::default();
        let t0 = Instant::now();
        cache.get_with(AgentKind::Claude, false, t0, || Ok(usage_with(10.0)));

        let t1 = t0 + FRESH_FOR;
        let limited = cache.get_with(AgentKind::Claude, true, t1, || {
            Err(Failure {
                message: "rate limited".into(),
                retry_after: Some(Duration::from_secs(600)),
            })
        });
        assert_eq!(limited.windows[0].used_percent, 10.0);
        assert_eq!(limited.error.as_deref(), Some("rate limited"));

        // Even a refresh waits out the backoff.
        let t2 = t1 + Duration::from_secs(300);
        cache.get_with(AgentKind::Claude, true, t2, || {
            panic!("fetched during backoff")
        });
        let t3 = t1 + Duration::from_secs(601);
        let ok = cache.get_with(AgentKind::Claude, true, t3, || Ok(usage_with(30.0)));
        assert_eq!((ok.windows[0].used_percent, ok.error), (30.0, None));
    }
}
