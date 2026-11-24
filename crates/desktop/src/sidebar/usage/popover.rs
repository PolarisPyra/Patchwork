//! The usage popover: each agent's windows, and its header and footer.

// Rust guideline compliant 2026-10-09

use super::limits::{BODY_H, GROUP_GAP, GROUP_H, NOTE_H, PANEL_PAD, PANEL_PAD_TOP, ROW_H};
use super::limits::describe_agent;
use super::limits::wait_left;
use super::limits::Entry;
use super::ring::ring;
use super::limits::most_used;
use super::limits::limited_until;
use gpui::{AnyElement, FontWeight, Role, SharedString, deferred, div, prelude::*, px, relative};
use patchwork_core::provider::AgentKind;
use patchwork_ui::theme::{fs_sm, fs_xs, radius, radius_sm, Theme};
use patchwork_ui::{
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
                .text_size(px(fs_sm()))
                .aria_expanded(open)
                .map(|b| {
                    if open {
                        b.bg(hover).text_color(fg)
                    } else {
                        b.text_color(t.text_muted)
                    }
                })
                // Kept while open too: GPUI only tracks the pointer for an element with a hover
                // style, and a stale "hovered" leaves the label bright after the popover closes.
                .hover(move |s| s.bg(hover).text_color(fg))
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
                .text_size(px(fs_sm()))
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
            let used = most_used(entry);
            let active = Some(kind) == current;
            let title = [
                Some(agent_label(kind).to_string()),
                used.map(|used| format!("{}% used", used.round())),
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
                .aria_label(title.clone())
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
                .child(ring(used, 34., 3., t, agent_logo(kind, None, 18.)))
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
        // Only the agent on show is drawn. Laid side by side in the body, every panel would take
        // a share of its width and squeeze the one on show; stacked in one grid cell, the hidden
        // ones would still set the column's width and push the panel out past the popover.
        let panel_usage = current
            .map(|kind| agent_usage(kind, w.usage.entries.get(&kind), t, now).into_any_element());
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
        // As tall as the tallest agent's limits, so switching tabs never resizes the popover.
        let body_h = agents
            .iter()
            .map(|kind| panel_height(w.usage.entries.get(kind)))
            .fold(BODY_H, f32::max);
        let body = div()
            .flex()
            .h(px(body_h))
            .border_t_1()
            .border_color(t.border)
            .child(
                div()
                    .id("usage-tabs")
                    .role(Role::TabList)
                    .aria_label("Agents")
                    .flex()
                    .flex_none()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .p(px(8.))
                    .border_r_1()
                    .border_color(t.border)
                    .bg(t.bg_app.opacity(0.4))
                    .children(tabs),
            )
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .children(panel_usage),
            );
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
                .text_size(px(fs_xs()))
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
                .text_size(px(fs_xs()))
                .text_color(t.text_subtle)
                .child("Up to date")
                .into_any_element(),
        )
    } else {
        usage
            .and_then(|u| u.plan.clone())
            .map(|plan| patchwork_ui::badge(plan, t).into_any_element())
    };
    div()
        .flex()
        .flex_none()
        .h(px(44.))
        .items_center()
        .gap(px(8.))
        .pl(px(16.))
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

/// One agent's limits: a ring for each limit, how much of it is used, and when it resets.
pub(super) fn agent_usage(
    kind: AgentKind,
    entry: Option<&Entry>,
    t: &Theme,
    now: u64,
) -> gpui::Stateful<gpui::Div> {
    let usage = entry.and_then(|e| e.usage.as_ref());
    // A provider that splits its quota by model family names the family once, as agy does,
    // rather than on every window under it.
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut family: Option<&str> = None;
    for window in usage.into_iter().flat_map(|u| &u.windows) {
        if let Some(name) = window.group.as_deref().filter(|name| family != Some(*name)) {
            rows.push(group_row(name, family.is_some(), t));
            family = Some(name);
        }
        rows.push(window_row(window, t, now));
    }
    div()
        .id(SharedString::from(format!(
            "usage-panel-{}",
            agent_label(kind)
        )))
        .role(Role::TabPanel)
        .aria_label(agent_label(kind))
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .pt(px(PANEL_PAD_TOP))
        .pb(px(PANEL_PAD - PANEL_PAD_TOP))
        .children(note(entry).map(|(note, why)| {
            div()
                .id("usage-note")
                .flex()
                .flex_none()
                .items_end()
                .h(px(NOTE_H))
                .px(px(16.))
                .pb(px(4.))
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .when_some(why, |row, why| {
                    row.aria_label(format!("{note}: {why}"))
                        .tooltip(tooltip(why))
                })
                .child(note)
        }))
        .children(rows)
}

/// What stands above an agent's limits instead of or beside them: loading, why the agent
/// cannot be asked, or that its last check failed, with the reason it gave to show on hover.
fn note(entry: Option<&Entry>) -> Option<(String, Option<String>)> {
    let usage = entry.and_then(|e| e.usage.as_ref());
    let status = entry.and_then(|e| e.status.as_ref());
    // A status that has not been read yet says nothing against the numbers already known.
    let ready = status.is_none_or(|s| s.installed && s.signed_in);
    // A rate limit's error reads "trying again in about N min", which the badge counts down
    // instead, and which is out of date once the wait is over.
    let rate_limited = usage.is_some_and(|u| u.retry_at.is_some());
    match entry {
        None => Some(("Loading…".to_string(), None)),
        Some(_) if !ready => Some((describe_agent(status), None)),
        Some(_) if rate_limited => None,
        Some(_) => usage.and_then(|u| match &u.error {
            Some(error) => Some(("Can't display limits".to_string(), Some(error.clone()))),
            None => u
                .windows
                .is_empty()
                .then(|| ("No limits reported".to_string(), None)),
        }),
    }
}

/// The height `agent_usage` draws for `entry`: its note, family headings and rows, padded.
fn panel_height(entry: Option<&Entry>) -> f32 {
    let windows = entry
        .and_then(|e| e.usage.as_ref())
        .map_or(&[][..], |u| u.windows.as_slice());
    let mut families = 0_u8;
    let mut family: Option<&str> = None;
    for name in windows.iter().filter_map(|w| w.group.as_deref()) {
        if family != Some(name) {
            families += 1;
            family = Some(name);
        }
    }
    let note = if note(entry).is_some() { NOTE_H } else { 0. };
    PANEL_PAD
        + note
        + f32::from(families) * GROUP_H
        + f32::from(families.saturating_sub(1)) * GROUP_GAP
        + windows.len() as f32 * ROW_H
}

/// The models a family of limits covers, above the windows that share it. Its name sits low,
/// near its own windows, and a hairline parts it from the family `after` it follows.
fn group_row(name: &str, after: bool, t: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_end()
        .h(px(GROUP_H))
        .px(px(16.))
        .pb(px(4.))
        .when(after, |row| {
            row.mt(px(GROUP_GAP)).border_t_1().border_color(t.border)
        })
        .text_size(px(fs_xs()))
        .text_color(t.text_muted)
        .child(div().min_w_0().truncate().child(name.to_string()))
        .into_any_element()
}

/// One limit: a ring of what is used, its name, and when it resets.
fn window_row(window: &patchwork_core::usage::UsageWindow, t: &Theme, now: u64) -> AnyElement {
    let used = window.used_percent.clamp(0., 100.);
    let resets = window.resets_at.map(|at| resets_in(at, now));
    let label = match (&window.group, &resets) {
        (Some(group), Some(resets)) => {
            format!(
                "{group} · {}: {}% used, resets {resets}",
                window.label,
                used.round()
            )
        }
        (Some(group), None) => {
            format!("{group} · {}: {}% used", window.label, used.round())
        }
        (None, Some(resets)) => {
            format!("{}: {}% used, resets {resets}", window.label, used.round())
        }
        (None, None) => format!("{}: {}% used", window.label, used.round()),
    };
    div()
        .id(SharedString::from(format!("usage-window-{}", window.label)))
        .role(Role::ListItem)
        .aria_label(label)
        .flex()
        .flex_none()
        .h(px(ROW_H))
        .items_center()
        .gap(px(12.))
        .px(px(16.))
        .child(ring(
            Some(used),
            40.,
            3.5,
            t,
            div()
                .text_size(px(patchwork_ui::theme::fs(11.)))
                .font_weight(FontWeight::MEDIUM)
                .child(format!("{}%", used.round())),
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(
                    div()
                        .truncate()
                        .text_size(px(fs_sm()))
                        .child(window.label.clone()),
                )
                .children(resets.map(|resets| {
                    div()
                        .truncate()
                        .text_size(px(fs_xs()))
                        .text_color(t.text_subtle)
                        .child(format!("Resets {resets}"))
                })),
        )
        .into_any_element()
}
