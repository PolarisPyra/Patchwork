//! What moves in place, the spinner, and the one clock every animation steps by.

// Rust guideline compliant 2026-10-09

use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::TAU;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use gpui::{App, EntityId, Hsla, PathBuilder, Role, Window, canvas, div, point, prelude::*, px};

/// The steady spinner.
///
/// A faint ring with a quarter arc running around it once every 0.8s. Every arc's lap counts from
/// the app's start, so all spinners turn in step, however late each one appears. Nothing rotates,
/// so the ring keeps its pixels.
pub fn spinner(size: f32, color: Hsla) -> impl IntoElement {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    let ring = canvas(
        |_, _, _| {},
        move |bounds, (), window: &mut Window, cx: &mut App| {
            next_step(window, cx);
            let scale = size / 24.;
            let center = bounds.center();
            let radius = 9. * scale;
            let width = px(3. * scale);
            let at = |turn: f32| {
                let angle = turn * TAU;
                point(
                    center.x + px(radius * angle.cos()),
                    center.y + px(radius * angle.sin()),
                )
            };
            let mut ring = PathBuilder::stroke(width);
            ring.move_to(at(0.));
            ring.arc_to(point(px(radius), px(radius)), px(0.), false, true, at(0.5));
            ring.arc_to(point(px(radius), px(radius)), px(0.), false, true, at(0.));
            if let Ok(path) = ring.build() {
                window.paint_path(path, color.opacity(0.25));
            }
            let lap = (start.elapsed().as_secs_f32() / 0.8).fract();
            let (from, to) = (lap, lap + 0.25);
            let mut arc = PathBuilder::stroke(width);
            arc.move_to(at(from));
            arc.arc_to(point(px(radius), px(radius)), px(0.), false, true, at(to));
            if let Ok(path) = arc.build() {
                window.paint_path(path, color);
            }
            // The arc's round caps.
            for end in [from, to] {
                let c = at(end);
                let r = px(1.5 * scale);
                let mut cap = PathBuilder::fill();
                cap.move_to(point(c.x + r, c.y));
                cap.arc_to(point(r, r), px(0.), false, true, point(c.x - r, c.y));
                cap.arc_to(point(r, r), px(0.), false, true, point(c.x + r, c.y));
                if let Ok(path) = cap.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .size(px(size))
    .flex_none();
    div()
        .id("spinner")
        .role(Role::ProgressIndicator)
        .aria_label("Loading")
        .flex_none()
        .child(ring)
}

/// How often what moves in place, a spinner's arc or a card's shimmer, moves on.
const MOTION_FPS: f32 = 60.;

thread_local! {
    /// The views that have a frame coming, and when.
    static PENDING: RefCell<HashMap<EntityId, Instant>> = RefCell::new(HashMap::new());
}

/// Seconds on the clock everything that moves steps by.
pub fn motion_clock() -> f32 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

/// Draws the view being painted again after `wait`, once for everything in it that moves: a
/// frame already coming sooner covers this one.
///
/// The frame lands on the next of the clock's steps, `MOTION_FPS` a second, so every motion in
/// the window moves in the same frames instead of each adding its own.
pub fn redraw_after(wait: Duration, window: &mut Window, cx: &App) {
    let view = window.current_view();
    let now = motion_clock();
    let step = ((now + wait.as_secs_f32()) * MOTION_FPS).floor() + 1.;
    let wait = Duration::from_secs_f32((step / MOTION_FPS - now).max(0.));
    let due = Instant::now() + wait;
    let sooner = PENDING.with(|p| {
        let mut pending = p.borrow_mut();
        match pending.get(&view) {
            Some(&at) if at <= due => false,
            _ => {
                pending.insert(view, due);
                true
            }
        }
    });
    if !sooner {
        return;
    }
    let timer = cx.background_executor().timer(wait);
    window
        .spawn(cx, async move |cx| {
            timer.await;
            PENDING.with(|p| {
                let mut pending = p.borrow_mut();
                if pending.get(&view).is_some_and(|&at| at <= due) {
                    pending.remove(&view);
                }
            });
            let _ = cx.update(|_, cx| cx.notify(view));
        })
        .detach();
}

/// Draws the view being painted again at the clock's next step.
///
/// Asking for the next frame instead would draw the whole view at the screen's rate, up to
/// 120 times a second.
pub fn next_step(window: &mut Window, cx: &App) {
    redraw_after(Duration::ZERO, window, cx);
}

/// Whether the element `handle` tracks scrolls, as last laid out. When layout changes that, the
/// view draws once more, so a scrollbar and the room kept for it follow content that grew or shrank.
pub fn overflows<V: 'static>(
    handle: &gpui::ScrollHandle,
    window: &mut Window,
    cx: &mut gpui::Context<V>,
) -> bool {
    let seen = (handle.bounds().size.height, handle.max_offset().y);
    let handle = handle.clone();
    cx.on_next_frame(window, move |_, _, cx| {
        if (handle.bounds().size.height, handle.max_offset().y) != seen {
            cx.notify();
        }
    });
    seen.1 > px(0.)
}
