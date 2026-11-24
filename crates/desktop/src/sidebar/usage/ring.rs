//! The ring in the sidebar that shows how much of a limit is used.
//!
//! The ring is empty while the window is untouched and fills as it is used, so a nearly full
//! ring is a nearly spent limit.

// Rust guideline compliant 2026-11-24

use std::f32::consts::TAU;
use gpui::{Hsla, PathBuilder, Window, canvas, div, point, prelude::*, px};
use patchwork_ui::theme::Theme;

/// A limit counts as nearly spent at this much used, and as running low from the second.
const NEARLY_SPENT: f32 = 0.90;
const RUNNING_LOW: f32 = 0.70;

/// A circular gauge showing `used` percent of a limit, with `child` in its middle.
pub(super) fn ring(
    used: Option<f64>,
    size: f32,
    stroke: f32,
    t: &Theme,
    child: impl IntoElement,
) -> impl IntoElement {
    let share = (used.unwrap_or(0.) / 100.).clamp(0., 1.) as f32;
    let track = t.border_strong;
    let fill: Hsla = if share >= NEARLY_SPENT {
        t.danger
    } else if share >= RUNNING_LOW {
        t.warning
    } else {
        t.accent
    };
    let gauge = canvas(
        |_, _, _| {},
        move |bounds, (), window: &mut Window, _| {
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
            // The track is laid in two halves: an arc that ends where it starts is dropped whole
            // by the path builder, which leaves the gauge with no ring around it.
            let mut circle = PathBuilder::stroke(px(stroke));
            circle.move_to(at(0.));
            circle.arc_to(radii, px(0.), false, true, at(0.5));
            circle.arc_to(radii, px(0.), false, true, at(0.));
            if let Ok(path) = circle.build() {
                window.paint_path(path, track);
            }
            if share <= 0. {
                return;
            }
            // A whole turn cannot be drawn as an arc, so a limit nothing has touched keeps the
            // track alone rather than a fill that would come out as a stub.
            let end = share.min(0.9999);
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
