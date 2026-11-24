//! Which agents' usage is shown, what is known of each, and opening, loading and closing the popover.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use gpui::{App, FocusHandle, KeyDownEvent, Pixels, Point, Window, prelude::*};
use patchwork_core::provider::{AgentKind, AgentStatus, detect_agent};
use patchwork_core::usage::ProviderUsage;
use patchwork_ui::now_ms;
use crate::workspace::Workspace;

/// Every agent the popover can show: the ones with plan limits (Claude, Codex, Antigravity), then
/// OpenCode, when they are connected in Settings → Providers.
pub(super) const SHOWN_AGENTS: [AgentKind; 4] = [
    AgentKind::Claude,
    AgentKind::Codex,
    AgentKind::Antigravity,
    AgentKind::OpenCode,
];
/// The height of each limit's row.
pub(super) const ROW_H: f32 = 52.;
/// The height of the heading over a family of limits, such as "Gemini Models".
pub(super) const GROUP_H: f32 = 24.;
/// The limits area's least height, so switching agents never resizes the popover: room for
/// agy's two families of two windows each, the most any agent reports, with 4px above them, 8px
/// below and 4px between.
pub(super) const BODY_H: f32 = 2. * GROUP_H + 4. * ROW_H + GROUP_GAP + PANEL_PAD;
/// The height of the line saying why an agent shows no numbers; it sits where a family's heading
/// would.
pub(super) const NOTE_H: f32 = GROUP_H;
/// The space between one family of limits and the next.
pub(super) const GROUP_GAP: f32 = 4.;
/// The limits' padding above them, and above and below together.
pub(super) const PANEL_PAD_TOP: f32 = 4.;
pub(super) const PANEL_PAD: f32 = 12.;
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

/// The most used across an agent's limits, or `None` before numbers arrive.
pub(super) fn most_used(entry: Option<&Entry>) -> Option<f64> {
    let windows = &entry?.usage.as_ref()?.windows;
    windows
        .iter()
        .map(|w| w.used_percent.clamp(0., 100.))
        .reduce(f64::max)
}

/// What "Up to date" compares: each limit's name and how much of it is left, to the whole
/// percent the rows show.
fn numbers(usage: &ProviderUsage) -> Vec<(String, i64)> {
    let mut numbers: Vec<(String, i64)> = usage
        .windows
        .iter()
        .map(|w| (w.label.clone(), w.remaining_percent().round() as i64))
        .collect();
    numbers.sort();
    numbers
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
            patchwork_core::provider::api::providers()
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

    /// Reads every shown agent's usage at startup, so the popover opens on numbers already known
    /// rather than on agy's seconds-long `/usage` probe.
    pub(crate) fn preload_usage(&mut self, cx: &mut Context<Self>) {
        let shown = self.shown_usage_agents();
        self.load_usage(false, &shown, cx);
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
            // What the core already knows shows at once, so the popover never opens empty
            // while a probe runs: the numbers it brings back replace these.
            if !refresh
                && self
                    .usage
                    .entries
                    .get(&kind)
                    .is_none_or(|entry| entry.usage.is_none())
                && let Some(known) = self.core.usage.known(kind)
            {
                self.usage.entries.insert(
                    kind,
                    Entry {
                        status: None,
                        usage: Some(known),
                    },
                );
            }
            if !self.usage.pending.insert(kind) {
                continue;
            }
            let core = Arc::clone(&self.core);
            patchwork_ui::off_thread(
                cx,
                move || {
                    // agy's `/usage` reports a missing sign-in itself, so it runs beside agy's
                    // slow sign-in handshake rather than after it.
                    if kind == AgentKind::Antigravity {
                        let (status, usage) = std::thread::scope(|s| {
                            let usage = s.spawn(|| core.usage.get(kind, refresh));
                            (detect_agent(kind), usage.join())
                        });
                        return Entry {
                            usage: usage.ok().filter(|_| status.installed && status.signed_in),
                            status: Some(status),
                        };
                    }
                    let status = detect_agent(kind);
                    let usage = (status.installed && status.signed_in)
                        .then(|| core.usage.get(kind, refresh));
                    Entry {
                        status: Some(status),
                        usage,
                    }
                },
                move |this, entry, cx| {
                    // A refresh asks the provider again, so "Up to date" says it answered with
                    // the same numbers rather than that nothing was asked.
                    let before = this
                        .usage
                        .entries
                        .get(&kind)
                        .and_then(|e| e.usage.as_ref())
                        .map(numbers);
                    let after = entry.usage.as_ref().map(numbers);
                    let limited = limited_until(Some(&entry), now_ms()).is_some();
                    if refresh && !limited && before.is_some() && before == after {
                        this.usage.up_to_date = Some(kind);
                        patchwork_ui::after(cx, UP_TO_DATE, move |this, cx| {
                            if this.usage.up_to_date == Some(kind) {
                                this.usage.up_to_date = None;
                                cx.notify();
                            }
                        });
                    }
                    this.usage.entries.insert(kind, entry);
                    this.usage.pending.remove(&kind);
                    cx.notify();
                },
            );
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
