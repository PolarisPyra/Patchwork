//! The ring in the sidebar that shows how much of a limit is used.

// Rust guideline compliant 2026-10-09

use std::f32::consts::TAU;
use gpui::{Hsla, PathBuilder, Window, canvas, div, point, prelude::*, px};
use crate::ui::theme::Theme;

/// A circular gauge filled to `percent` from the top, clockwise, with `child` in its middle.
pub(super) fn ring(
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
