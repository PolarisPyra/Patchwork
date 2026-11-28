//! The sidebar footer's Usage button and its popover.
//!
//! Every connected agent is listed at once, each limit a slim bar with how much is used and when
//! it starts over, so one glance answers "how close am I" for all of them without switching tabs.

// Rust guideline compliant 2026-10-09

use patchwork_client::AppState;
use patchwork_views_logos::{agent_label, agent_logo};
use patchwork_platform::now_ms;
use gpui::{
    AnyElement, Window, FontWeight, Role, SharedString, deferred, div, prelude::*, px, relative,
};
use patchwork_model::provider::AgentKind;
use patchwork_client::local::core::usage::UsageWindow;
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, theme, fs, fs_sm, radius_lg, radius_sm};
use patchwork_design_components::{button, popup_shadow, spinner, tooltip};
use patchwork_design_text::format_date;
use patchwork_design_icons::icon;

use super::limits::{
    Entry, RUNNING_LOW, ago, describe_agent, duration, fill_colour, limited_until, wait_left,
};
use super::limits::Usage;

/// The popover's least width, so a narrow sidebar does not squeeze the limits' names away.
const MIN_W: f32 = 260.;
/// The popover's greatest height; more agents than fit scroll inside it.
const MAX_H: f32 = 520.;
/// The height of an agent's heading row and of the button in the footer.
const ROW_H: f32 = 28.;
/// The width a used limit's bar keeps at the least, so a sliver of use still shows.
const MIN_FILL: f64 = 2.;

impl Render for Usage {
    /// The footer: the Usage button, with its popover above it while open.
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        footer(self, &t, cx)
    }
}

/// The footer: the Usage button, with its popover above it while open.
fn footer(w: &mut Usage, t: &Theme, cx: &mut Context<Usage>) -> AnyElement {
    let fg = t.text;
    let hover = t.bg_hover;
    let open = w.open;
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
                .h(px(ROW_H))
                .gap(px(8.))
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
                .on_click(cx.listener(Usage::toggle))
                .child(div().flex_none().child(icon(IconName::Gauge, 16.)))
                .child("Usage"),
        )
        .children(popover)
        .into_any_element()
}

/// The popover over the footer: a section per connected agent, or why there are none.
fn popover(w: &mut Usage, t: &Theme, cx: &mut Context<Usage>) -> AnyElement {
    let now = now_ms();
    let shown = w.shown_agents(cx);
    let agents: Vec<AgentKind> = shown
        .iter()
        .copied()
        .filter(|kind| {
            w.entries
                .get(kind)
                .and_then(|e| e.status.as_ref())
                .is_none_or(|s| s.installed)
        })
        .collect();
    if agents
        .iter()
        .any(|&kind| limited_until(w.entries.get(&kind), now).is_some())
    {
        // Counts the rate limit's wait down.
        w.tick_each_second(cx);
    }
    let body: Vec<AnyElement> = if agents.is_empty() {
        let connected_api =
            w.api_usable && AppState::get(cx).settings.agents.contains(&AgentKind::Api);
        vec![
            div()
                .px(px(8.))
                .py(px(8.))
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .child(if !shown.is_empty() || connected_api {
                    "Unable to get usage stats"
                } else {
                    "No agents connected"
                })
                .into_any_element(),
        ]
    } else {
        agents
            .iter()
            .enumerate()
            .map(|(i, &kind)| {
                let state = SectionState {
                    first: i == 0,
                    busy: w.pending.contains(&kind),
                    up_to_date: w.up_to_date == Some(kind),
                };
                agent_section(kind, w.entries.get(&kind), state, now, t, cx)
            })
            .collect()
    };
    let panel = div()
        .id("usage-popover")
        .role(Role::Dialog)
        .aria_label("Usage")
        .track_focus(&w.focus)
        .on_key_down(cx.listener(Usage::on_key))
        .occlude()
        .on_mouse_down_out(cx.listener(|w, e: &gpui::MouseDownEvent, _, cx| {
            w.dismissed = Some(e.position);
            w.close(cx);
        }))
        .min_w(px(MIN_W))
        .overflow_hidden()
        .rounded(px(radius_lg()))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_elevated)
        .shadow(popup_shadow(t))
        .text_color(t.text)
        .child(
            div()
                .id("usage-scroll")
                .flex()
                .flex_col()
                .max_h(px(MAX_H))
                .overflow_y_scroll()
                .p(px(4.))
                .children(body),
        );
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

/// What an agent's section shows besides its numbers.
#[derive(Clone, Copy)]
struct SectionState {
    /// The first section has no rule above it.
    first: bool,
    /// Its numbers are being read; the refresh button spins.
    busy: bool,
    /// Its last refresh brought nothing new; "Up to date" stands in for the plan.
    up_to_date: bool,
}

/// One agent: its name, its plan or rate limit, when it last reported and its refresh button,
/// then its limits, a family heading above each family of them.
fn agent_section(
    kind: AgentKind,
    entry: Option<&Entry>,
    state: SectionState,
    now: u64,
    t: &Theme,
    cx: &mut Context<Usage>,
) -> AnyElement {
    let name = agent_label(kind);
    let usage = entry.and_then(|e| e.usage.as_ref());
    let status = entry.and_then(|e| e.status.as_ref());
    // A status that has not been read yet says nothing against the numbers already known.
    let ready = status.is_none_or(|s| s.installed && s.signed_in);
    let limited = limited_until(entry, now);
    let refresh_label = limited.map_or_else(
        || "Refresh".to_string(),
        |at| format!("Rate limited, try again in {}", wait_left(at, now)),
    );
    let badge = if let Some(at) = limited {
        Some(
            div()
                .id(SharedString::from(format!("usage-limit-{name}")))
                .role(Role::Status)
                .flex_none()
                .rounded(px(radius_sm()))
                .bg(t.danger_soft)
                .px(px(6.))
                .py(px(1.))
                .text_size(px(fs(10.5)))
                .text_color(t.danger)
                .child(format!("Rate limited · {}", wait_left(at, now)))
                .into_any_element(),
        )
    } else if state.up_to_date {
        Some(
            div()
                .id(SharedString::from(format!("usage-fresh-{name}")))
                .role(Role::Status)
                .flex_none()
                .text_size(px(fs(11.)))
                .text_color(t.text_subtle)
                .child("Up to date")
                .into_any_element(),
        )
    } else {
        usage.and_then(|u| u.plan.as_deref()).map(|plan| {
            div()
                .flex_none()
                .rounded(px(radius_sm()))
                .bg(t.bg_selected_muted)
                .px(px(6.))
                .py(px(1.))
                .text_size(px(fs(10.5)))
                .text_color(t.text_muted)
                .child(capitalized(plan))
                .into_any_element()
        })
    };
    let reported = usage
        .and_then(|u| u.updated_at)
        .filter(|_| !state.busy)
        .map(|at| {
            let secs = i64::try_from(at / 1000).unwrap_or(i64::MAX);
            div()
                .id(SharedString::from(format!("usage-ago-{name}")))
                .flex_none()
                .text_size(px(fs(10.5)))
                .text_color(t.text_subtle)
                .tooltip(tooltip(format!("Last reported {}", format_date(secs))))
                .child(ago(at, now))
        });
    let refresh = ready.then(|| {
        let (hover, fg) = (t.bg_hover, t.text);
        button(
            SharedString::from(format!("usage-refresh-{name}")),
            refresh_label.clone(),
        )
        .size(px(ROW_H))
        .flex_none()
        .justify_center()
        .rounded(px(5.))
        .text_color(t.text_subtle)
        .hover(move |s| s.bg(hover).text_color(fg))
        .tooltip(tooltip(refresh_label))
        .map(|b| {
            if state.busy {
                b.child(spinner(12., t.text_subtle))
            } else {
                b.child(icon(IconName::ArrowsClockwise, 12.))
            }
        })
        .on_click(cx.listener(move |w, _, _, cx| {
            if !w.pending.contains(&kind) {
                w.load(true, &[kind], cx);
            }
        }))
    });
    let header = div()
        .flex()
        .h(px(ROW_H))
        .items_center()
        .gap(px(8.))
        .pl(px(8.))
        .child(agent_logo(kind, None, 14.))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(fs_sm()))
                .font_weight(FontWeight::MEDIUM)
                .child(name),
        )
        .children(badge)
        .child(div().flex_1())
        .children(reported)
        .children(refresh);

    // A provider that splits its quota by model family names the family once, as agy does,
    // rather than on every window under it.
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut family: Option<&str> = None;
    for window in usage.into_iter().flat_map(|u| &u.windows) {
        if let Some(group) = window.group.as_deref().filter(|g| family != Some(*g)) {
            rows.push(
                div()
                    .truncate()
                    .px(px(8.))
                    .pt(px(6.))
                    .text_size(px(fs(11.)))
                    .text_color(t.text_subtle)
                    .child(group.to_string())
                    .into_any_element(),
            );
            family = Some(group);
        }
        rows.push(limit_row(window, now, t));
    }

    div()
        .id(SharedString::from(format!("usage-agent-{name}")))
        .role(Role::Group)
        .aria_label(name)
        .flex_none()
        .pb(px(4.))
        .when(!state.first, |s| {
            s.mt(px(4.)).pt(px(4.)).border_t_1().border_color(t.border)
        })
        .child(header)
        .children(note(entry).map(|(note, why)| {
            div()
                .id(SharedString::from(format!("usage-note-{name}")))
                .truncate()
                .px(px(8.))
                .pb(px(2.))
                .text_size(px(fs(11.5)))
                .text_color(t.text_muted)
                .when_some(why, |row, why| {
                    row.aria_label(format!("{note}: {why}"))
                        .tooltip(tooltip(why))
                })
                .child(note)
        }))
        .when(!rows.is_empty(), |s| {
            s.child(div().id("usage-limits").role(Role::List).children(rows))
        })
        .into_any_element()
}

/// "Max", "Plus": a plan as the provider names it, capitalized as the badge shows it.
fn capitalized(plan: &str) -> String {
    let mut chars = plan.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// What stands below an agent's name instead of or beside its limits: loading, why the agent
/// cannot be asked, or that its last check failed, with the reason it gave to show on hover.
fn note(entry: Option<&Entry>) -> Option<(String, Option<String>)> {
    let usage = entry.and_then(|e| e.usage.as_ref());
    let status = entry.and_then(|e| e.status.as_ref());
    let ready = status.is_none_or(|s| s.installed && s.signed_in);
    // A rate limit's error reads "trying again in about N min", which the badge counts down
    // instead, and which is out of date once the wait is over.
    let rate_limited = usage.is_some_and(|u| u.retry_at.is_some() && !u.windows.is_empty());
    match entry {
        None => Some(("Loading…".to_string(), None)),
        Some(_) if !ready => Some((describe_agent(status), None)),
        Some(_) if rate_limited => None,
        Some(_) => usage.and_then(|u| match &u.error {
            Some(error) => Some(("Can't read limits".to_string(), Some(error.clone()))),
            None => u
                .windows
                .is_empty()
                .then(|| ("No limits reported".to_string(), None)),
        }),
    }
}

/// One limit: its name, when it starts over and how much of it is used, over a bar.
fn limit_row(window: &UsageWindow, now: u64, t: &Theme) -> AnyElement {
    let used = window.used_percent.clamp(0., 100.);
    let share = used / 100.;
    let colour = fill_colour(share, t);
    let resets = window.resets_at.map(|at| duration(at.saturating_sub(now)));
    let name = match &window.group {
        Some(group) => format!("{group} · {}", window.label),
        None => window.label.clone(),
    };
    let label = match &resets {
        Some(resets) => format!("{name}: {}% used, resets in {resets}", used.round()),
        None => format!("{name}: {}% used", used.round()),
    };
    let fill = if used > 0. { used.max(MIN_FILL) } else { 0. };
    div()
        .id(SharedString::from(format!("usage-window-{name}")))
        .role(Role::ListItem)
        .aria_label(label)
        .px(px(8.))
        .py(px(4.))
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(8.))
                .text_size(px(fs(11.5)))
                .line_height(px(16.))
                .child(div().min_w_0().truncate().child(window.label.clone()))
                .children(resets.map(|resets| {
                    div()
                        .flex_none()
                        .text_size(px(fs(10.5)))
                        .text_color(t.text_subtle)
                        .child(format!("resets in {resets}"))
                }))
                .child(
                    div()
                        .ml_auto()
                        .flex_none()
                        .font_weight(FontWeight::MEDIUM)
                        // The number takes the bar's warning colour once the limit runs low.
                        .when(share >= RUNNING_LOW, |n| n.text_color(colour))
                        .child(format!("{}%", used.round())),
                ),
        )
        .child(
            div()
                .mt(px(4.))
                .h(px(4.))
                .overflow_hidden()
                .rounded_full()
                .bg(t.bg_hover)
                .child(
                    div()
                        .h_full()
                        .w(relative((fill / 100.) as f32))
                        .rounded_full()
                        .bg(colour),
                ),
        )
        .into_any_element()
}
