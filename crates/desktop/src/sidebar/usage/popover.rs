//! The usage popover: each agent's windows, and its header and footer.

// Rust guideline compliant 2026-10-09

use super::state::ROW_H;
use super::state::describe_agent;
use super::state::wait_left;
use super::state::Entry;
use super::ring::ring;
use super::state::peak;
use super::state::limited_until;
use gpui::{AnyElement, FontWeight, Role, SharedString, deferred, div, prelude::*, px, relative};
use patchwork_server::provider::AgentKind;
use crate::ui::theme::{FS_SM, FS_XS, radius, radius_sm, Theme};
use crate::ui::{
    agent_label, agent_logo, button, icon, now_ms, popup_shadow, resets_in, spinner, tooltip,
};
use crate::workspace::Workspace;

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
                .rounded(px(radius_sm()))
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
                .on_click(cx.listener(crate::workspace::Workspace::toggle_usage))
                .child(icon("chart-bar", 18.))
                .child("Usage"),
        )
        .children(popover)
        .into_any_element()
}

pub(super) fn popover(w: &mut Workspace, t: &Theme, cx: &mut Context<Workspace>) -> AnyElement {
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
        .rounded(px(radius()))
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
                .rounded(px(radius_sm()))
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
            .size(px(28.))
            .flex_none()
            .justify_center()
            .rounded(px(radius_sm()))
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
            agent_usage(kind, w.usage.entries.get(&kind), t, now)
                .w_full()
                .flex_none()
                .when(i > 0, |p| p.ml(relative(-1.)))
                .when(Some(kind) != current, gpui::Styled::invisible)
        });
        let header = current.map(|kind| {
            usage_header(
                kind,
                w.usage.entries.get(&kind),
                now,
                w.usage.up_to_date == Some(kind),
                refresh.into_any_element(),
                t,
            )
        });
        let body = div()
            .flex()
            .child(
                div()
                    .id("usage-tabs")
                    .role(Role::TabList)
                    .aria_label("Agents")
                    .flex()
                    .flex_none()
                    .flex_col()
                    .items_center()
                    // A tab every 52px, as the limits' rows are, so each tab's ring is level
                    // with a limit's ring.
                    .gap(px(12.))
                    .px(px(8.))
                    .py(px(6.))
                    .border_t_1()
                    .border_r_1()
                    .border_color(t.border)
                    .bg(t.bg_app.opacity(0.4))
                    .children(tabs),
            )
            .child(div().flex().min_w_0().flex_1().children(panels));
        panel.flex_col().children(header).child(body)
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

/// The popover's top row: the agent being looked at, its plan or what its last refresh came to,
/// and the refresh button.
pub(super) fn usage_header(
    kind: AgentKind,
    entry: Option<&Entry>,
    now: u64,
    up_to_date: bool,
    refresh: AnyElement,
    t: &Theme,
) -> gpui::Div {
    let usage = entry.and_then(|e| e.usage.as_ref());
    let limit = limited_until(entry, now);
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
                .rounded(px(radius_sm()))
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
    div()
        .flex()
        .flex_none()
        .h(px(44.))
        .items_center()
        .gap(px(8.))
        .pl(px(14.))
        .pr(px(8.))
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .child(agent_label(kind)),
        )
        .children(badge)
        .child(div().flex_1())
        .child(refresh)
}

/// One agent's limits: a ring for each limit and when it resets.
pub(super) fn agent_usage(
    kind: AgentKind,
    entry: Option<&Entry>,
    t: &Theme,
    now: u64,
) -> gpui::Stateful<gpui::Div> {
    let usage = entry.and_then(|e| e.usage.as_ref());
    let status = entry.and_then(|e| e.status.as_ref());
    let ready = status.is_some_and(|s| s.installed && s.signed_in);
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
    let windows = usage.into_iter().flat_map(|u| &u.windows).map(|w| {
        div()
            .flex()
            .flex_none()
            .h(px(ROW_H))
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .border_t_1()
            .border_color(t.border)
            .child(ring(
                Some(w.used_percent),
                40.,
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
                    .flex_1()
                    .min_w_0()
                    .gap(px(1.))
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
        .children(note.map(|note| {
            div()
                .px(px(14.))
                .py(px(12.))
                .border_t_1()
                .border_color(t.border)
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
                .child(note)
        }))
        .children(windows)
}
