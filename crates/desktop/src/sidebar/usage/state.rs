//! Which agents' usage is shown, what is known of each, and opening, loading and closing the popover.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use gpui::{App, FocusHandle, KeyDownEvent, Pixels, Point, Window, prelude::*};
use patchwork_server::provider::{AgentKind, AgentStatus, detect_agent};
use patchwork_server::usage::ProviderUsage;
use crate::ui::now_ms;
use crate::workspace::Workspace;

/// Every agent the popover can show: the ones with plan limits (Claude, Codex, Antigravity), then
/// OpenCode, when they are connected in Settings → Providers.
pub(super) const SHOWN_AGENTS: [AgentKind; 4] = [
    AgentKind::Claude,
    AgentKind::Codex,
    AgentKind::Antigravity,
    AgentKind::OpenCode,
];
/// The height of each limit's row, and of the agent tab beside it.
pub(super) const ROW_H: f32 = 52.;
/// How long "Up to date" stands in for the plan badge before the badge comes back.
pub(super) const UP_TO_DATE: Duration = Duration::from_millis(1200);

/// What is known of one agent: whether it can be asked, and its numbers once read.
#[derive(Default)]
pub(super) struct Entry {
    pub(super) status: Option<AgentStatus>,
    pub(super) usage: Option<ProviderUsage>,
}

pub struct UsageState {
    pub(super) open: bool,
    pub(super) focus: FocusHandle,
    /// Where the press that last closed the popover was, so it does not open it again.
    pub(super) dismissed: Option<Point<Pixels>>,
    pub(super) entries: HashMap<AgentKind, Entry>,
    /// Agents whose usage is still being read; each one finishes on its own.
    pub(super) pending: HashSet<AgentKind>,
    pub(super) selected: Option<AgentKind>,
    /// The agent whose last refresh brought nothing new, while "Up to date" shows for it.
    pub(super) up_to_date: Option<AgentKind>,
    /// Whether an API provider is switched on and can run a task.
    pub(super) api_usable: bool,
}

impl UsageState {
    pub fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            focus: cx.focus_handle(),
            dismissed: None,
            entries: HashMap::new(),
            pending: HashSet::new(),
            selected: None,
            up_to_date: None,
            api_usable: false,
        }
    }
}

/// When the provider allows the next usage check, while it is rate-limiting them.
pub(super) fn limited_until(entry: Option<&Entry>, now: u64) -> Option<u64> {
    entry
        .and_then(|e| e.usage.as_ref()?.retry_at)
        .filter(|&at| at > now)
}

/// The highest used percentage across an agent's limits, or `None` before numbers arrive.
pub(super) fn peak(entry: Option<&Entry>) -> Option<f64> {
    let windows = &entry?.usage.as_ref()?.windows;
    windows.iter().map(|w| w.used_percent).reduce(f64::max)
}

/// "4m", "1h 5m": the wait left, rounded up so it never reads 0m.
pub(super) fn wait_left(at: u64, now: u64) -> String {
    let mins = at.saturating_sub(now).div_ceil(60000).max(1);
    if mins >= 60 {
        format!("{}h {}m", mins / 60, mins % 60)
    } else {
        format!("{mins}m")
    }
}

/// "Ready · 2.1.0", or why the agent cannot be asked (`describeAgent`).
pub(super) fn describe_agent(status: Option<&AgentStatus>) -> String {
    match status {
        None => "Unavailable".into(),
        Some(s) if !s.installed => {
            if s.detail.is_empty() {
                "Not installed".into()
            } else {
                s.detail.clone()
            }
        }
        Some(s) => {
            let state = if s.signed_in {
                "Ready"
            } else {
                "Sign-in required"
            };
            match &s.version {
                Some(v) => format!("{state} · {v}"),
                None => state.into(),
            }
        }
    }
}

impl Workspace {
    pub(super) fn toggle_usage(
        &mut self,
        e: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let closing_press = matches!(e, gpui::ClickEvent::Mouse(click)
            if self.usage.dismissed.take() == Some(click.down.position));
        if closing_press || self.usage.open {
            self.close_usage(window, cx);
            return;
        }
        self.usage.open = true;
        window.focus(&self.usage.focus, cx);
        let shown = self.shown_usage_agents();
        self.load_usage(false, &shown, cx);
        let task = cx.background_spawn(async {
            patchwork_server::provider::api::config::providers()
                .iter()
                .any(|p| p.enabled && !p.models.is_empty())
        });
        cx.spawn(async move |this, cx| {
            let usable = task.await;
            this.update(cx, |this, cx| {
                this.usage.api_usable = usable;
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    /// Closes the popover without taking the keys, for when they are going somewhere else.
    pub fn dismiss_usage(&mut self, cx: &mut Context<Self>) {
        if self.usage.open {
            self.usage.open = false;
            cx.notify();
        }
    }

    pub(super) fn close_usage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.usage.open = false;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn shown_usage_agents(&self) -> Vec<AgentKind> {
        SHOWN_AGENTS
            .into_iter()
            .filter(|kind| self.settings.agents.contains(kind))
            .collect()
    }

    /// Reads each agent's status and, when it is signed in, its numbers. An agent still being
    /// read is skipped, so a slow one is not asked twice.
    pub(super) fn load_usage(
        &mut self,
        refresh: bool,
        kinds: &[AgentKind],
        cx: &mut Context<Self>,
    ) {
        for &kind in kinds {
            if !self.usage.pending.insert(kind) {
                continue;
            }
            let server = Arc::clone(&self.server);
            let task = cx.background_spawn(async move {
                let status = detect_agent(kind);
                let usage =
                    (status.installed && status.signed_in).then(|| server.usage.get(kind, refresh));
                Entry {
                    status: Some(status),
                    usage,
                }
            });
            cx.spawn(async move |this, cx| {
                let entry = task.await;
                this.update(cx, |this, cx| {
                    // The server returns the last numbers when it skips a refresh, so the same
                    // reading time means nothing new came back.
                    let before = this
                        .usage
                        .entries
                        .get(&kind)
                        .and_then(|e| e.usage.as_ref()?.updated_at);
                    let after = entry.usage.as_ref().and_then(|u| u.updated_at);
                    let limited = limited_until(Some(&entry), now_ms()).is_some();
                    if refresh && !limited && before.is_some() && after == before {
                        this.usage.up_to_date = Some(kind);
                        cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(UP_TO_DATE).await;
                            this.update(cx, |this, cx| {
                                if this.usage.up_to_date == Some(kind) {
                                    this.usage.up_to_date = None;
                                    cx.notify();
                                }
                            })
                        })
                        .detach();
                    }
                    this.usage.entries.insert(kind, entry);
                    this.usage.pending.remove(&kind);
                    cx.notify();
                })
            })
            .detach();
        }
        cx.notify();
    }

    pub(super) fn on_usage_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if e.keystroke.key == "escape" {
            cx.stop_propagation();
            self.close_usage(window, cx);
        }
    }
}
