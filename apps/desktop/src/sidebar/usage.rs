//! The sidebar footer's Usage button and its popover (`features/usage/UsagePopover.tsx`): the plan
//! limits of each connected agent that has them, as rings. A tab per agent on the left, each
//! ringed by its highest limit, and a refresh button that reads only the agent being looked at.

use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;
use std::time::Duration;

use gpui::{
    AnyElement, App, Context, FocusHandle, FontWeight, Hsla, KeyDownEvent, PathBuilder, Pixels,
    Point, Role, SharedString, Window, canvas, deferred, div, point, prelude::*, px, relative,
};
use patchwork_server::provider::{AgentKind, AgentStatus, detect_agent};
use patchwork_server::usage::ProviderUsage;

use crate::ui::theme::{FS_SM, FS_XS, RADIUS, RADIUS_SM, Theme};
use crate::ui::{agent_label, agent_logo, button, icon, now_ms, popup_shadow, spinner, tooltip};
use crate::workspace::Workspace;

/// Every agent the popover can show: the ones with plan limits (Claude, Codex, Antigravity), then
/// OpenCode, when they are connected in Settings → Providers.
const SHOWN_AGENTS: [AgentKind; 4] = [
    AgentKind::Claude,
    AgentKind::Codex,
    AgentKind::Antigravity,
    AgentKind::OpenCode,
];
/// How long "Up to date" stands in for the plan badge before the badge comes back.
const UP_TO_DATE: Duration = Duration::from_millis(1200);

/// What is known of one agent: whether it can be asked, and its numbers once read.
#[derive(Default)]
struct Entry {
    status: Option<AgentStatus>,
    usage: Option<ProviderUsage>,
}

pub struct UsageState {
    open: bool,
    focus: FocusHandle,
    /// Where the press that last closed the popover was, so it does not open it again.
    dismissed: Option<Point<Pixels>>,
    entries: HashMap<AgentKind, Entry>,
    /// Agents whose usage is still being read; each one finishes on its own.
    pending: HashSet<AgentKind>,
    selected: Option<AgentKind>,
    /// The agent whose last refresh brought nothing new, while "Up to date" shows for it.
    up_to_date: Option<AgentKind>,
    /// Whether an API provider is switched on and can run a task.
    api_usable: bool,
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
fn limited_until(entry: Option<&Entry>, now: u64) -> Option<u64> {
    entry
        .and_then(|e| e.usage.as_ref()?.retry_at)
        .filter(|&at| at > now)
}

/// The highest used percentage across an agent's limits, or `None` before numbers arrive.
fn peak(entry: Option<&Entry>) -> Option<f64> {
    let windows = &entry?.usage.as_ref()?.windows;
    windows.iter().map(|w| w.used_percent).reduce(f64::max)
}

/// "in 3h 12m", "in 2d 4h", "in 5m".
fn resets_in(at: u64, now: u64) -> String {
    let mins = (at.saturating_sub(now) as f64 / 60000.).round() as u64;
    let (d, h, m) = (mins / 1440, mins % 1440 / 60, mins % 60);
    if d > 0 {
        format!("in {d}d {h}h")
    } else if h > 0 {
        format!("in {h}h {m}m")
    } else {
        format!("in {m}m")
    }
}

/// "4m", "1h 5m": the wait left, rounded up so it never reads 0m.
fn wait_left(at: u64, now: u64) -> String {
    let mins = at.saturating_sub(now).div_ceil(60000).max(1);
    if mins >= 60 {
        format!("{}h {}m", mins / 60, mins % 60)
    } else {
        format!("{mins}m")
    }
}

/// "Ready · 2.1.0", or why the agent cannot be asked (`describeAgent`).
fn describe_agent(status: Option<&AgentStatus>) -> String {
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

/// A circular gauge filled to `percent` from the top, clockwise, with `child` in its middle.
fn ring(
    percent: Option<f64>,
    size: f32,
    stroke: f32,
    t: &Theme,
    child: impl IntoElement,
) -> impl IntoElement {
    let p = percent.unwrap_or(0.).clamp(0., 100.) as f32;
    let track = t.border_strong;
    let fill: Hsla = if p >= 90. {
        t.danger
    } else if p >= 70. {
        t.warning
    } else {
        t.accent
    };
    let gauge = canvas(
        |_, _, _| {},
        move |bounds, _, window: &mut Window, _| {
            let center = bounds.center();
            let r = (size - stroke) / 2.;
            let at = |turn: f32| {
                let angle = (turn - 0.25) * TAU;
                point(
                    center.x + px(r * angle.cos()),
                    center.y + px(r * angle.sin()),
                )
            };
            let radii = point(px(r), px(r));
            let mut circle = PathBuilder::stroke(px(stroke));
            circle.move_to(at(0.));
            circle.arc_to(radii, px(0.), false, true, at(0.5));
            circle.arc_to(radii, px(0.), false, true, at(0.));
            if let Ok(path) = circle.build() {
                window.paint_path(path, track);
            }
            if p <= 0. {
                return;
            }
            let end = (p / 100.).min(0.9999);
            let mut arc = PathBuilder::stroke(px(stroke));
            arc.move_to(at(0.));
            arc.arc_to(radii, px(0.), end > 0.5, true, at(end));
            if let Ok(path) = arc.build() {
                window.paint_path(path, fill);
            }
            // The arc's round caps.
            for turn in [0., end] {
                let c = at(turn);
                let cap_r = px(stroke / 2.);
                let mut cap = PathBuilder::fill();
                cap.move_to(point(c.x + cap_r, c.y));
                cap.arc_to(
                    point(cap_r, cap_r),
                    px(0.),
                    false,
                    true,
                    point(c.x - cap_r, c.y),
                );
                cap.arc_to(
                    point(cap_r, cap_r),
                    px(0.),
                    false,
                    true,
                    point(c.x + cap_r, c.y),
                );
                if let Ok(path) = cap.build() {
                    window.paint_path(path, fill);
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size(px(size));
    div()
        .relative()
        .flex_none()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(gauge)
        .child(child)
}

impl Workspace {
    fn toggle_usage(&mut self, e: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
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

    fn close_usage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.usage.open = false;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn shown_usage_agents(&self) -> Vec<AgentKind> {
        SHOWN_AGENTS
            .into_iter()
            .filter(|kind| self.settings.agents.contains(kind))
            .collect()
    }

    /// Reads each agent's status and, when it is signed in, its numbers. An agent still being
    /// read is skipped, so a slow one is not asked twice.
    fn load_usage(&mut self, refresh: bool, kinds: &[AgentKind], cx: &mut Context<Self>) {
        for &kind in kinds {
            if !self.usage.pending.insert(kind) {
                continue;
            }
            let server = self.server.clone();
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

    fn on_usage_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if e.keystroke.key == "escape" {
            cx.stop_propagation();
            self.close_usage(window, cx);
        }
    }
}

/// The footer: the Usage button, with its popover above it while open.
pub fn footer(w: &mut Workspace, t: &Theme, cx: &mut Context<Workspace>) -> AnyElement {
    let fg = t.text;
    let hover = t.bg_hover;
    let open = w.usage.open;
    let popover = open.then(|| popover(w, t, cx));
    div()
        .relative()
        .flex_none()
        .py(px(6.))
        .px(px(8.))
        .border_t_1()
        .border_color(t.border)
        .child(
            button("usage", "Usage")
                .w_full()
                .h(px(28.))
                .gap(px(6.))
                .px(px(8.))
                .rounded(px(RADIUS_SM))
                .text_size(px(FS_SM))
                .aria_expanded(open)
                .map(|b| {
                    if open {
                        b.bg(hover).text_color(fg)
                    } else {
                        b.text_color(t.text_muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                    }
                })
                .on_click(cx.listener(|w, e, window, cx| w.toggle_usage(e, window, cx)))
                .child(icon("chart-bar", 18.))
                .child("Usage"),
        )
        .children(popover)
        .into_any_element()
}

fn popover(w: &mut Workspace, t: &Theme, cx: &mut Context<Workspace>) -> AnyElement {
    let now = now_ms();
    let shown = w.shown_usage_agents();
    let agents: Vec<AgentKind> = shown
        .iter()
        .copied()
        .filter(|kind| {
            w.usage
                .entries
                .get(kind)
                .and_then(|e| e.status.as_ref())
                .is_none_or(|s| s.installed)
        })
        .collect();
    if agents
        .iter()
        .any(|&kind| limited_until(w.usage.entries.get(&kind), now).is_some())
    {
        // Counts the rate limit's wait down.
        w.tick_each_second(cx);
    }
    let current = w
        .usage
        .selected
        .filter(|kind| agents.contains(kind))
        .or(agents.first().copied());
    let panel = div()
        .id("usage-popover")
        .role(Role::Dialog)
        .aria_label("Usage")
        .track_focus(&w.usage.focus)
        .on_key_down(cx.listener(Workspace::on_usage_key))
        .occlude()
        .on_mouse_down_out(cx.listener(|w, e: &gpui::MouseDownEvent, window, cx| {
            w.usage.dismissed = Some(e.position);
            w.close_usage(window, cx);
        }))
        .flex()
        .overflow_hidden()
        .rounded(px(RADIUS))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_elevated)
        .shadow(popup_shadow(t))
        .text_color(t.text);
    let panel = if agents.is_empty() {
        let connected_api = w.usage.api_usable && w.settings.agents.contains(&AgentKind::Api);
        panel.child(
            div()
                .p(px(12.))
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
                .child(if !shown.is_empty() || connected_api {
                    "Unable to get usage stats"
                } else {
                    "No agents connected"
                }),
        )
    } else {
        let current_limited = current
            .and_then(|k| limited_until(w.usage.entries.get(&k), now))
            .is_some();
        let busy = current.is_some_and(|k| w.usage.pending.contains(&k));
        let refresh_label = if current_limited {
            "Rate limited"
        } else {
            "Refresh"
        };
        let tabs = agents.iter().map(|&kind| {
            let entry = w.usage.entries.get(&kind);
            let top = peak(entry);
            let active = Some(kind) == current;
            let title = [
                Some(agent_label(kind).to_string()),
                top.map(|p| format!("{}% used", p.round())),
                limited_until(entry, now).map(|_| "Rate limited".to_string()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            let soft = t.bg_hover.opacity(0.6);
            div()
                .id(SharedString::from(format!(
                    "usage-tab-{}",
                    agent_label(kind)
                )))
                .role(Role::Tab)
                .aria_label(agent_label(kind))
                .aria_selected(active)
                .tab_index(0)
                .cursor_pointer()
                .size(px(40.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(RADIUS_SM))
                .map(|tab| {
                    if active {
                        tab.bg(t.bg_hover)
                    } else {
                        tab.hover(move |s| s.bg(soft))
                    }
                })
                .tooltip(tooltip(title))
                .on_click(cx.listener(move |w, _, _, cx| {
                    w.usage.selected = Some(kind);
                    cx.notify();
                }))
                .child(ring(top, 34., 3., t, agent_logo(kind, None, 18.)))
        });
        let refresh = button("usage-refresh", refresh_label)
            .mt_auto()
            .w(px(40.))
            .h(px(28.))
            .justify_center()
            .rounded(px(RADIUS_SM))
            .text_color(t.text_muted)
            .hover({
                let (hover, fg) = (t.bg_hover, t.text);
                move |s| s.bg(hover).text_color(fg)
            })
            .tooltip(tooltip(refresh_label))
            .map(|b| {
                if busy {
                    b.child(spinner(14., t.text_muted))
                } else {
                    b.child(icon("arrows-clockwise", 14.))
                }
            })
            .on_click(cx.listener(move |w, _, _, cx| {
                if let Some(kind) = current {
                    w.load_usage(true, &[kind], cx);
                }
            }));
        // Every panel shares one place, so the popover keeps the tallest one's height and the
        // tabs stay put while switching.
        let panels = agents.iter().enumerate().map(|(i, &kind)| {
            agent_usage(
                kind,
                w.usage.entries.get(&kind),
                now,
                w.usage.up_to_date == Some(kind),
                t,
            )
            .w_full()
            .flex_none()
            .when(i > 0, |p| p.ml(relative(-1.)))
            .when(Some(kind) != current, |p| p.invisible())
        });
        panel
            .child(
                div()
                    .id("usage-tabs")
                    .role(Role::TabList)
                    .aria_label("Agents")
                    .flex()
                    .flex_none()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .p(px(8.))
                    .border_r_1()
                    .border_color(t.border)
                    .bg(t.bg_app.opacity(0.4))
                    .children(tabs)
                    .child(refresh),
            )
            .child(div().flex().min_w_0().flex_1().children(panels))
    };
    deferred(
        div()
            .absolute()
            .left(px(8.))
            .right(px(8.))
            .bottom(relative(1.))
            .pb(px(10.))
            .child(panel),
    )
    .with_priority(1)
    .into_any_element()
}

/// One agent's limits: its name and plan, then a ring for each limit and when it resets.
fn agent_usage(
    kind: AgentKind,
    entry: Option<&Entry>,
    now: u64,
    up_to_date: bool,
    t: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let usage = entry.and_then(|e| e.usage.as_ref());
    let status = entry.and_then(|e| e.status.as_ref());
    let ready = status.is_some_and(|s| s.installed && s.signed_in);
    let limit = limited_until(entry, now);
    // A rate limit's error reads "trying again in about N min", which the badge counts down
    // instead, and which is out of date once the wait is over.
    let rate_limited = usage.is_some_and(|u| u.retry_at.is_some());
    let note = match entry {
        None => Some("Loading…".to_string()),
        Some(_) if !ready => Some(describe_agent(status)),
        Some(_) if rate_limited => None,
        Some(_) => usage.and_then(|u| {
            u.error.clone().or_else(|| {
                u.windows
                    .is_empty()
                    .then(|| "No limits reported".to_string())
            })
        }),
    };
    let badge = if let Some(at) = limit {
        Some(
            div()
                .id(SharedString::from(format!(
                    "usage-limit-{}",
                    agent_label(kind)
                )))
                .role(Role::Status)
                .aria_label("Rate limited")
                .h(px(20.))
                .flex()
                .items_center()
                .px(px(6.))
                .rounded(px(RADIUS_SM))
                .bg(t.danger_soft)
                .text_color(t.danger)
                .text_size(px(FS_XS))
                .child(format!("Rate limited · {}", wait_left(at, now)))
                .into_any_element(),
        )
    } else if up_to_date {
        Some(
            div()
                .id(SharedString::from(format!(
                    "usage-fresh-{}",
                    agent_label(kind)
                )))
                .role(Role::Status)
                .h(px(20.))
                .flex()
                .items_center()
                .text_size(px(FS_XS))
                .text_color(t.text_subtle)
                .child("Up to date")
                .into_any_element(),
        )
    } else {
        usage
            .and_then(|u| u.plan.clone())
            .map(|plan| crate::ui::badge(plan, t).into_any_element())
    };
    let windows = usage.into_iter().flat_map(|u| &u.windows).map(|w| {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(ring(
                Some(w.used_percent),
                42.,
                3.5,
                t,
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("{}%", w.used_percent.round())),
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(div().truncate().text_size(px(FS_SM)).child(w.label.clone()))
                    .children(w.resets_at.map(|at| {
                        div()
                            .truncate()
                            .text_size(px(FS_XS))
                            .text_color(t.text_subtle)
                            .child(format!("Resets {}", resets_in(at, now)))
                    })),
            )
    });
    div()
        .id(SharedString::from(format!(
            "usage-panel-{}",
            agent_label(kind)
        )))
        .role(Role::TabPanel)
        .aria_label(agent_label(kind))
        .flex()
        .flex_col()
        .min_w_0()
        .gap(px(10.))
        .p(px(10.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .pb(px(10.))
                .border_b_1()
                .border_color(t.border)
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(agent_label(kind)),
                )
                .children(badge),
        )
        .children(note.map(|note| {
            div()
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
                .child(note)
        }))
        .children(windows)
}
