//! Fetching each agent's usage, and the cache that throttles it.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::provider::AgentKind;
use crate::system::now_ms;

use super::{ProviderUsage, UsageWindow, antigravity, claude, codex};
use super::types::SESSION_LABEL;

/// How long fetched usage is reused before it is fetched again, for a read that did not ask for
/// fresh numbers.
const FRESH_FOR: Duration = Duration::from_secs(5 * 60);
/// Minimum time between fetches of the limits a turn's reports leave out, which nothing asked
/// for. An explicit refresh is not held to it: the person pressed it.
const MIN_REFRESH: Duration = Duration::from_secs(60);
/// Wait after a 429 that did not say how long to wait.
pub(super) const RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(5 * 60);

/// Why usage couldn't be read.
#[derive(Debug)]
pub(super) struct Failure {
    pub(super) message: String,
    /// Set when the provider asked us to slow down.
    pub(super) retry_after: Option<Duration>,
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

impl From<crate::Error> for Failure {
    fn from(error: crate::Error) -> Self {
        error.to_string().into()
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
        retry_at: None,
    }
}

fn fetch(kind: AgentKind) -> Result<ProviderUsage, Failure> {
    match kind {
        AgentKind::Claude => claude::usage(empty(kind)),
        AgentKind::Codex => codex::usage(empty(kind)).map_err(Failure::from),
        AgentKind::Antigravity => antigravity::usage(empty(kind)).map_err(Failure::from),
        // OpenCode bills through the providers it is signed in to and reports no plan limits.
        AgentKind::OpenCode => Ok(ProviderUsage {
            error: Some("OpenCode has no plan limits to show".into()),
            ..empty(kind)
        }),
        AgentKind::Api => Ok(ProviderUsage {
            error: Some("API providers bill per token and have no plan limits to show".into()),
            ..empty(kind)
        }),
    }
}

#[derive(Debug)]
struct Entry {
    fetched: Instant,
    /// The numbers came from the agent itself during a turn, not from a fetch.
    reported: bool,
    usage: ProviderUsage,
    /// When the provider was last asked. A turn's report resets `fetched`, not this.
    asked: Option<Instant>,
    /// Whether the provider has been asked at all. An agent reports only some of the limits
    /// during a turn: a limit for one model, such as Fable, comes only from asking.
    complete: bool,
    /// No requests before this, after a rate limit.
    blocked_until: Option<Instant>,
}

#[derive(Debug, Default)]
pub struct UsageCache {
    entries: Mutex<HashMap<AgentKind, Entry>>,
    /// The agents being fetched now. A second caller meanwhile gets the cached numbers, so a
    /// rate-limited endpoint is never asked twice at once. Locked inside `entries`.
    in_flight: Mutex<HashSet<AgentKind>>,
}

impl UsageCache {
    /// Drops cached usage for `kind` so the next read fetches fresh numbers.
    pub fn invalidate(&self, kind: AgentKind) {
        self.entries.locked().remove(&kind);
    }

    /// The usage last known for `kind`, without asking the provider. None before any is known.
    #[must_use]
    pub fn known(&self, kind: AgentKind) -> Option<ProviderUsage> {
        let entries = self.entries.locked();
        entries.get(&kind).map(|e| e.usage.clone())
    }

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
        let lock = || self.entries.locked();
        let flying = || self.in_flight.locked();
        let entries = lock();
        if let Some(e) = entries.get(&kind) {
            let age = now.saturating_duration_since(e.fetched);
            let blocked = e.blocked_until.is_some_and(|until| now < until);
            // A refresh is the person asking for the numbers again, so it asks: only a provider
            // that rate-limited the last check holds it back. Opening the popup does not ask
            // while what is known is still fresh.
            let keep = if e.complete {
                !refresh && age < FRESH_FOR
            } else {
                // Known only from a turn's reports so far: ask once, without waiting for a
                // refresh, for the limits the reports leave out.
                e.asked
                    .is_some_and(|at| now.saturating_duration_since(at) < MIN_REFRESH)
            };
            if blocked || keep {
                return e.usage.clone();
            }
        }
        if !flying().insert(kind) {
            return entries
                .get(&kind)
                .map_or_else(|| empty(kind), |e| e.usage.clone());
        }
        drop(entries);
        let result = fetch();
        let mut entries = lock();
        flying().remove(&kind);
        // Numbers the agent reported while this fetched are newer than what it brought back,
        // but the fetch has the limits a report leaves out.
        if let Some(e) = entries
            .get_mut(&kind)
            .filter(|e| e.reported && e.fetched > now)
        {
            if let Ok(fetched) = result {
                for window in fetched.windows {
                    if !e.usage.windows.iter().any(|w| w.label == window.label) {
                        e.usage.windows.push(window);
                    }
                }
                e.complete = true;
            }
            e.asked = Some(now);
            return e.usage.clone();
        }
        let complete = entries.get(&kind).is_some_and(|e| e.complete);
        let entry = match result {
            Ok(usage) => Entry {
                fetched: now,
                reported: false,
                usage,
                asked: Some(now),
                complete: true,
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
                let wait = failure.retry_after.map(|d| d.max(MIN_REFRESH));
                usage.retry_at = wait.map(|d| now_ms() + d.as_millis() as u64);
                Entry {
                    fetched: now,
                    reported: false,
                    usage,
                    asked: Some(now),
                    complete,
                    blocked_until: wait.map(|d| now + d),
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
        let mut entries = self.entries.locked();
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
                asked: previous.as_ref().and_then(|e| e.asked),
                complete: previous.as_ref().is_some_and(|e| e.complete),
                blocked_until: previous.and_then(|e| e.blocked_until),
            },
        );
    }
}

/// "Session (5h)" for a 300-minute window, "Weekly" for 10080, and so on.
pub(super) fn window_label(minutes: u64) -> String {
    match minutes {
        300 => SESSION_LABEL.into(),
        10_080 => "Weekly".into(),
        m if m % 1440 == 0 => format!("{}-day", m / 1440),
        m => format!("{}h", (m as f64 / 60.0).round()),
    }
}
