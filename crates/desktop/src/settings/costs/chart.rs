//! The chart: a bar of cost for each day of the range, or each month of all time. A bar picked
//! by the pointer or the arrow keys has its figures shown above the bars.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, ElementId, Hsla, KeyDownEvent, Role, SharedString, div, prelude::*, px};
use patchwork_core::costs::{Bucket, CostReport, Tally};
use patchwork_core::provider::AgentKind;

use super::rows::{dollars, series};
use crate::settings::SettingsDialog;
use crate::ui::{agent_label, format_tokens};
use crate::ui::theme::{Theme, fs_sm, fs_xs, radius};

/// A bar's period: `Nov 19`, or `Nov 2026` for a month.
fn period(report: &CostReport, b: &Bucket) -> String {
    if report.monthly {
        b.start.format("%b %Y").to_string()
    } else {
        b.start.format("%b %-d").to_string()
    }
}

fn figures(tally: &Tally) -> String {
    format!(
        "{} · {} tokens · {} replies",
        dollars(tally.cost()),
        format_tokens(tally.tokens()),
        tally.replies
    )
}

/// A bar's figures, then each provider's part when there is more than one.
fn bar_figures(report: &CostReport, b: &Bucket, by_cost: bool) -> String {
    let mut out = format!("{}: {}", period(report, b), figures(&b.tally));
    if b.by_agent.len() > 1 {
        let parts: Vec<String> = b
            .by_agent
            .iter()
            .map(|(agent, tally)| {
                let value = if by_cost {
                    dollars(tally.cost())
                } else {
                    format!("{} tokens", format_tokens(tally.tokens()))
                };
                format!("{} {value}", agent_label(*agent))
            })
            .collect();
        out.push_str(" — ");
        out.push_str(&parts.join(", "));
    }
    out
}

/// A provider's chart colour, by its place in the report's providers.
fn color_of(report: &CostReport, agent: AgentKind, t: &Theme) -> Hsla {
    let ix = report
        .providers
        .iter()
        .position(|p| p.agent == agent)
        .unwrap_or(0);
    series(ix, t)
}

pub(super) fn chart(
    report: &CostReport,
    picked: Option<usize>,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let by_cost = report.buckets.iter().any(|b| b.tally.cost() > 0.);
    let value = |tally: &Tally| {
        if by_cost {
            tally.cost()
        } else {
            tally.tokens() as f64
        }
    };
    let top = report
        .buckets
        .iter()
        .map(|b| value(&b.tally))
        .fold(0., f64::max);
    let picked = picked.filter(|&ix| ix < report.buckets.len());
    // The picked bar's figures, else the peak's.
    let shown = match picked.and_then(|ix| report.buckets.get(ix)) {
        Some(b) => bar_figures(report, b, by_cost),
        None if by_cost => format!("Peak {}", dollars(top)),
        None => format!("Peak {} tokens", format_tokens(top as u64)),
    };
    let idle = t.bg_hover;
    let bars = report.buckets.iter().enumerate().map(|(ix, b)| {
        // The first provider sits at the bottom of the stack.
        let segments: Vec<(f32, Hsla)> = b
            .by_agent
            .iter()
            .rev()
            .map(|(agent, tally)| {
                let h = if top > 0. { value(tally) / top } else { 0. };
                (h as f32, color_of(report, *agent, t))
            })
            .filter(|(h, _)| *h > 0.)
            .collect();
        let dim = picked.is_some_and(|p| p != ix);
        div()
            .id(ElementId::Integer(ix as u64))
            .role(Role::ListItem)
            .aria_label(SharedString::from(bar_figures(report, b, by_cost)))
            .aria_selected(picked == Some(ix))
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .justify_end()
            .when(dim, |d| d.opacity(0.45))
            .on_hover(cx.listener(move |d, over: &bool, _, cx| {
                if *over {
                    d.costs.bar = Some(ix);
                } else if d.costs.bar == Some(ix) {
                    d.costs.bar = None;
                }
                cx.notify();
            }))
            .when(segments.is_empty(), |d| {
                // A period without replies keeps a sliver, so the axis reads as periods.
                d.child(
                    div()
                        .w_full()
                        .rounded_t(px(2.))
                        .bg(idle)
                        .h(gpui::relative(0.02)),
                )
            })
            .children(segments.into_iter().enumerate().map(|(i, (h, color))| {
                div()
                    .w_full()
                    .flex_none()
                    .when(i == 0, |d| d.rounded_t(px(2.)))
                    .bg(color)
                    .h(gpui::relative(h))
            }))
    });
    let legend = (report.providers.len() > 1).then(|| {
        div()
            .id("costs-legend")
            .role(Role::List)
            .aria_label("Providers")
            .flex()
            .flex_wrap()
            .gap_x(px(12.))
            .gap_y(px(4.))
            .text_size(px(fs_xs()))
            .text_color(t.text_muted)
            .children(report.providers.iter().enumerate().map(|(ix, p)| {
                div()
                    .id(ElementId::Integer(ix as u64))
                    .role(Role::ListItem)
                    .aria_label(agent_label(p.agent))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(div().size(px(8.)).rounded(px(2.)).bg(series(ix, t)))
                    .child(agent_label(p.agent))
            }))
    });
    let n = report.buckets.len();
    let first = report.buckets.first().map(|b| period(report, b));
    let last = report.buckets.last().map(|b| {
        if report.monthly {
            period(report, b)
        } else {
            "Today".into()
        }
    });
    let what = if by_cost { "Cost" } else { "Tokens" };
    let per = if report.monthly { "month" } else { "day" };
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .p(px(12.))
        .rounded(px(radius()))
        .border_1()
        .border_color(t.border)
        .child(
            div()
                .id("costs-picked")
                .role(Role::Status)
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .child(shown),
        )
        .child(
            div()
                .id("costs-bars")
                .role(Role::List)
                .aria_label(SharedString::from(format!("{what} per {per}")))
                .tab_index(0)
                .focus_visible(crate::ui::focus_ring)
                .rounded(px(2.))
                .flex()
                .items_end()
                .gap(px(if n > 45 { 1. } else { 2. }))
                .h(px(96.))
                .on_key_down(cx.listener(move |d, e: &KeyDownEvent, _, cx| {
                    if n == 0 {
                        return;
                    }
                    let at = d.costs.bar.filter(|&ix| ix < n);
                    let next = match e.keystroke.key.as_str() {
                        "left" => at.map_or(n - 1, |ix| ix.saturating_sub(1)),
                        "right" => at.map_or(n - 1, |ix| (ix + 1).min(n - 1)),
                        "home" => 0,
                        "end" => n - 1,
                        "escape" if at.is_some() => {
                            d.costs.bar = None;
                            cx.stop_propagation();
                            cx.notify();
                            return;
                        }
                        _ => return,
                    };
                    d.costs.bar = Some(next);
                    cx.stop_propagation();
                    cx.notify();
                }))
                .children(bars),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .text_size(px(fs_xs()))
                .text_color(t.text_subtle)
                .children(first)
                .children(last),
        )
        .children(legend)
        .into_any_element()
}
