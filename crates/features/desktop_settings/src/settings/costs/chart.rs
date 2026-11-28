//! The bar chart: cost or tokens for each day of the range, or each month of all time, stacked by
//! provider over a scale. A bar picked by the pointer or the arrow keys shows its figures in a
//! card beside it.

// Rust guideline compliant 2026-10-09

use patchwork_views_logos::agent_label;
use gpui::{
    AnyElement, Div, ElementId, FontWeight, Hsla, KeyDownEvent, Role, SharedString, Stateful, div,
    prelude::*, px, relative,
};
use patchwork_client::local::core::costs::{Bucket, CostReport};

use super::rows::{amount, series, value};
use crate::settings::SettingsDialog;
use patchwork_design_theme::{Theme, fs_sm, fs_xs, radius};

/// The plot's height, without its labels.
const PLOT: f32 = 132.;
/// The width of the scale's labels at the left.
const AXIS: f32 = 44.;

/// A bar's period: `Nov 19`, or `Nov 2026` for a month.
pub(super) fn period(report: &CostReport, b: &Bucket) -> String {
    if report.monthly {
        b.start.format("%b %Y").to_string()
    } else {
        b.start.format("%b %-d").to_string()
    }
}

/// The top of the scale: `top` rounded up to 1, 2, 2.5 or 5 times a power of ten, so its half
/// is a round figure too.
pub(super) fn scale_top(top: f64) -> f64 {
    if top <= 0. {
        return 1.;
    }
    let power = 10f64.powf(top.log10().floor());
    let step = [1., 2., 2.5, 5., 10.]
        .into_iter()
        .find(|s| s * power >= top)
        .unwrap_or(10.);
    step * power
}

/// A dot of `color` before `name`, as the legends and the card show a provider.
pub(super) fn swatch(name: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .min_w_0()
        .child(div().flex_none().size(px(7.)).rounded_full().bg(color))
        .child(div().min_w_0().truncate().child(name.into()))
}

/// Each provider in its colour, when there is more than one.
pub(super) fn legend(id: &'static str, report: &CostReport, t: &Theme) -> Option<AnyElement> {
    (report.providers.len() > 1).then(|| {
        div()
            .id(id)
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
                    .child(swatch(agent_label(p.agent), series(p.agent, t)))
            }))
            .into_any_element()
    })
}

/// The scale at the left of a plot `height` high, and its lines across the plot: the top, its
/// half, and zero.
pub(super) fn scale(top: f64, by_cost: bool, height: f32, t: &Theme) -> (Div, Div) {
    let marks = [(0., top), (0.5, top / 2.), (1., 0.)];
    let labels = div()
        .relative()
        .flex_none()
        .w(px(AXIS))
        .h(px(height))
        .text_size(px(fs_xs()))
        .text_color(t.text_subtle)
        .children(marks.iter().map(|&(at, v)| {
            div()
                .absolute()
                .right(px(8.))
                .top(relative(at))
                .mt(px(-7.))
                .child(amount(v, by_cost))
        }));
    let lines = div()
        .absolute()
        .inset_0()
        .children(marks.iter().map(|&(at, _)| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(relative(at))
                .h(px(1.))
                .bg(t.border)
        }));
    (labels, lines)
}

/// The first, middle and last periods under the plot.
pub(super) fn periods(report: &CostReport, t: &Theme) -> Div {
    let n = report.buckets.len();
    let label = |ix: usize| report.buckets.get(ix).map(|b| period(report, b));
    let (first, middle, last) = match n {
        0 => (None, None, None),
        1 => (label(0), None, None),
        2 => (label(0), None, label(1)),
        _ => (label(0), label(n / 2), label(n - 1)),
    };
    div()
        .flex()
        .justify_between()
        .pl(px(AXIS))
        .text_size(px(fs_xs()))
        .text_color(t.text_subtle)
        .children(first)
        .children(middle)
        .children(last)
}

/// What a bar holds, as a screen reader reads it.
fn spoken(report: &CostReport, b: &Bucket, by_cost: bool) -> String {
    let mut out = format!(
        "{}: {}, {} replies",
        period(report, b),
        amount(value(&b.tally, by_cost), by_cost),
        b.tally.replies
    );
    if b.by_agent.len() > 1 {
        let parts: Vec<String> = b
            .by_agent
            .iter()
            .map(|(agent, tally)| {
                format!(
                    "{} {}",
                    agent_label(*agent),
                    amount(value(tally, by_cost), by_cost)
                )
            })
            .collect();
        out.push_str(". ");
        out.push_str(&parts.join(", "));
    }
    out
}

/// The picked bar's card: its period, each provider's part, and the total.
fn card(report: &CostReport, ix: usize, b: &Bucket, by_cost: bool, t: &Theme) -> Stateful<Div> {
    let n = report.buckets.len().max(1);
    // The card sits right of a bar in the left half, and left of one in the right half.
    let at = (ix as f32 + 0.5) / n as f32;
    let line = |name: Div, figure: String| {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .child(name)
            .child(div().flex_none().text_color(t.text).child(figure))
    };
    div()
        .id("costs-picked")
        .role(Role::Status)
        .aria_label(SharedString::from(spoken(report, b, by_cost)))
        .absolute()
        .top(px(4.))
        .map(|d| {
            if at < 0.5 {
                d.left(relative(at)).ml(px(10.))
            } else {
                d.right(relative(1. - at)).mr(px(10.))
            }
        })
        .w(px(170.))
        .flex()
        .flex_col()
        .gap(px(4.))
        .p(px(10.))
        .rounded(px(radius()))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_elevated)
        .shadow_md()
        .text_size(px(fs_xs()))
        .text_color(t.text_muted)
        .child(
            div()
                .text_color(t.text)
                .font_weight(FontWeight::MEDIUM)
                .child(period(report, b)),
        )
        .children(b.by_agent.iter().map(|(agent, tally)| {
            line(
                swatch(agent_label(*agent), series(*agent, t)),
                amount(value(tally, by_cost), by_cost),
            )
        }))
        .child(div().h(px(1.)).bg(t.border))
        .child(
            line(
                div().child("Total"),
                amount(value(&b.tally, by_cost), by_cost),
            )
            .font_weight(FontWeight::MEDIUM),
        )
        .child(div().child(format!("{} replies", b.tally.replies)))
}

/// A figure under the chart: its name, then its value.
fn fact(name: &str, figure: String, t: &Theme) -> Div {
    div()
        .flex()
        .gap(px(6.))
        .min_w_0()
        .child(
            div()
                .flex_none()
                .text_color(t.text_subtle)
                .child(name.to_string()),
        )
        .child(div().min_w_0().truncate().child(figure))
}

pub(super) fn chart(
    report: &CostReport,
    picked: Option<usize>,
    by_cost: bool,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let n = report.buckets.len();
    let top = scale_top(
        report
            .buckets
            .iter()
            .map(|b| value(&b.tally, by_cost))
            .fold(0., f64::max),
    );
    let picked = picked.filter(|&ix| ix < n);
    let idle = t.bg_hover;
    let bars = report.buckets.iter().enumerate().map(|(ix, b)| {
        // The first provider sits at the bottom of the stack.
        let segments: Vec<(f32, Hsla)> = b
            .by_agent
            .iter()
            .rev()
            .map(|(agent, tally)| ((value(tally, by_cost) / top) as f32, series(*agent, t)))
            .filter(|(h, _)| *h > 0.)
            .collect();
        let dim = picked.is_some_and(|p| p != ix);
        div()
            .id(ElementId::Integer(ix as u64))
            .role(Role::ListItem)
            .aria_label(SharedString::from(spoken(report, b, by_cost)))
            .aria_selected(picked == Some(ix))
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .justify_end()
            .when(dim, |d| d.opacity(0.4))
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
                d.child(div().w_full().rounded_t(px(2.)).bg(idle).h(relative(0.015)))
            })
            .children(segments.into_iter().enumerate().map(|(i, (h, color))| {
                div()
                    .w_full()
                    .flex_none()
                    .when(i == 0, |d| d.rounded_t(px(2.)))
                    .bg(color)
                    .h(relative(h))
            }))
    });
    let (labels, lines) = scale(top, by_cost, PLOT, t);
    let what = if by_cost { "cost" } else { "tokens" };
    let per = if report.monthly { "Monthly" } else { "Daily" };
    let title = format!("{per} {what}");
    let shown = picked
        .and_then(|ix| report.buckets.get(ix).map(|b| (ix, b)))
        .map(|(ix, b)| card(report, ix, b, by_cost, t));
    let plot = div()
        .id("costs-bars")
        .role(Role::List)
        .aria_label(SharedString::from(title.clone()))
        .tab_index(0)
        .focus_visible(patchwork_design_components::focus_ring)
        .relative()
        .flex_1()
        .min_w_0()
        .h(px(PLOT))
        .child(lines)
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_end()
                .gap(px(if n > 45 { 1. } else { 3. }))
                .children(bars),
        )
        .children(shown)
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
        }));
    let unit = if report.monthly { "month" } else { "day" };
    let peak = report.peak(by_cost).map_or_else(
        || "—".into(),
        |b| {
            format!(
                "{} · {}",
                period(report, b),
                amount(value(&b.tally, by_cost), by_cost)
            )
        },
    );
    let average = if by_cost {
        amount(report.daily_cost, true)
    } else {
        amount(report.daily_tokens as f64, false)
    };
    let facts = div()
        .flex()
        .flex_wrap()
        .gap_x(px(20.))
        .gap_y(px(4.))
        .pt(px(4.))
        .text_size(px(fs_sm()))
        .child(fact(&format!("Peak {unit}"), peak, t))
        .child(fact("Daily average", average, t))
        .child(fact(
            &format!("Active {unit}s"),
            format!("{} of {n}", report.active_periods()),
            t,
        ));
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .flex_1()
        .min_w_0()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(fs_sm()))
                        .font_weight(FontWeight::MEDIUM)
                        .child(title),
                )
                .children(legend("costs-legend", report, t)),
        )
        .child(div().flex().child(labels).child(plot))
        .child(periods(report, t))
        .child(facts)
        .into_any_element()
}
