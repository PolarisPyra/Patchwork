//! What moves in place, the spinner, and the one clock every animation steps by.

// Rust guideline compliant 2026-10-09

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use gpui::component::spinner::Spinner;
use gpui::component::{Sizable, Size as KitSize};
use gpui::{App, EntityId, Hsla, Role, Window, div, prelude::*, px};

/// gpui-component's spinner, `size` pixels across in `color`.
#[must_use]
pub fn spinner(size: f32, color: Hsla) -> impl IntoElement {
    div()
        .id("spinner")
        .role(Role::ProgressIndicator)
        .aria_label("Loading")
        .flex_none()
        .child(
            Spinner::new()
                .color(color)
                .with_size(KitSize::Size(px(size))),
        )
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
