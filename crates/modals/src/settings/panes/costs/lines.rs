//! Each provider's cost or tokens per period on a line of its own, filled below, so their shapes
//! can be compared.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, Hsla, PathBuilder, Pixels, Point, Role, SharedString, Window, canvas, div,
    linear_color_stop, linear_gradient, point, prelude::*, px,
};
use patchwork_core::costs::CostReport;

use super::chart::{legend, periods, scale, scale_top};
use super::rows::{amount, series, value};
use patchwork_ui::agent_label;
use patchwork_ui::theme::{Theme, fs_sm};

/// The plot's height, without its labels.
const PLOT: f32 = 120.;

/// A smooth path on from the first of `points`, where the path is, through the rest: each corner
/// becomes a curve through the middles of its sides.
fn trace(path: &mut PathBuilder, points: &[Point<Pixels>]) {
    let Some((first, rest)) = points.split_first() else {
        return;
    };
    let mut last = *first;
    for p in rest {
        let mid = point((last.x + p.x) / 2., (last.y + p.y) / 2.);
        path.curve_to(mid, last);
        last = *p;
    }
    path.line_to(last);
}

pub(super) fn lines(report: &CostReport, by_cost: bool, t: &Theme) -> Option<AnyElement> {
    let n = report.buckets.len();
    if report.providers.len() < 2 || n < 2 {
        return None;
    }
    // Each provider's value in each period, with its colour; the first provider is drawn last,
    // on top.
    let series_of: Vec<(Vec<f64>, Hsla)> = report
        .providers
        .iter()
        .rev()
        .map(|p| {
            let values = report
                .buckets
                .iter()
                .map(|b| {
                    b.by_agent
                        .iter()
                        .find(|(agent, _)| *agent == p.agent)
                        .map_or(0., |(_, tally)| value(tally, by_cost))
                })
                .collect();
            (values, series(p.agent, t))
        })
        .collect();
    let top = scale_top(
        series_of
            .iter()
            .flat_map(|(v, _)| v.iter().copied())
            .fold(0., f64::max),
    );
    let (labels, grid) = scale(top, by_cost, PLOT, t);
    let plot = canvas(
        |_, _, _| {},
        move |bounds, (), window: &mut Window, _| {
            let (x0, y1) = (bounds.origin.x, bounds.origin.y + bounds.size.height);
            let step = bounds.size.width / (n - 1) as f32;
            let h = f32::from(bounds.size.height);
            for (values, color) in &series_of {
                let points: Vec<Point<Pixels>> = values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| point(x0 + step * i as f32, y1 - px(h * (v / top) as f32)))
                    .collect();
                let mut area = PathBuilder::fill();
                area.move_to(point(x0, y1));
                area.line_to(points[0]);
                trace(&mut area, &points);
                area.line_to(point(x0 + bounds.size.width, y1));
                area.close();
                if let Ok(path) = area.build() {
                    window.paint_path(
                        path,
                        linear_gradient(
                            180.,
                            linear_color_stop(color.opacity(0.28), 0.),
                            linear_color_stop(color.opacity(0.), 1.),
                        ),
                    );
                }
                let mut line = PathBuilder::stroke(px(1.5));
                line.move_to(points[0]);
                trace(&mut line, &points);
                if let Ok(path) = line.build() {
                    window.paint_path(path, *color);
                }
            }
        },
    )
    .absolute()
    .inset_0();
    let spoken: Vec<String> = report
        .providers
        .iter()
        .map(|p| {
            format!(
                "{} {}",
                agent_label(p.agent),
                amount(value(&p.in_range, by_cost), by_cost)
            )
        })
        .collect();
    let what = if by_cost { "Cost" } else { "Tokens" };
    let title = format!("{what} by provider");
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(16.))
            .rounded(px(patchwork_ui::theme::radius_lg()))
            .border_1()
            .border_color(t.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(fs_sm()))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(title.clone()),
                    )
                    .children(legend("costs-lines-legend", report, t)),
            )
            .child(
                div().flex().child(labels).child(
                    div()
                        .id("costs-lines")
                        .role(Role::Image)
                        .aria_label(SharedString::from(format!(
                            "{title}: {}",
                            spoken.join(", ")
                        )))
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .h(px(PLOT))
                        .child(grid)
                        .child(plot),
                ),
            )
            .child(periods(report, t))
            .into_any_element(),
    )
}
