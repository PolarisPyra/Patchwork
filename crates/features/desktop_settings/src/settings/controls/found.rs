//! The setting a search result opened: where its row was drawn, so the pane can scroll it to the
//! middle, and the fading flash that marks it once it is there.

// Rust guideline compliant 2026-10-09

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};
use gpui::{Bounds, Div, Hsla, Pixels, ScrollHandle, Stateful, Window, canvas, point, prelude::*, px};

/// How long a found row stays lit while it fades.
///
/// Long enough for the eye to land on it after the pane scrolls, short enough that it is gone
/// before the user starts reading the next row.
pub const FLASH: Duration = Duration::from_millis(1400);

/// How many frames the pane waits for the sought row to be drawn before giving up on centering
/// it. A page switch draws its rows on the first frame, so a few are plenty.
const TRIES: u8 = 4;

thread_local! {
    /// The row a search result opened, by its name, until the pane has scrolled to it.
    static SOUGHT: RefCell<Option<&'static str>> = const { RefCell::new(None) };
    /// Where the sought row was last drawn, in window coordinates.
    static SPOT: Cell<Option<Bounds<Pixels>>> = const { Cell::new(None) };
    /// The row lit by a search, with its color and when it was found.
    static LIT: RefCell<Option<(&'static str, Hsla, Instant)>> = const { RefCell::new(None) };
}

/// Marks the row named `name` to be found, lit in `color`; None forgets the last one.
pub fn seek(sought: Option<(&'static str, Hsla)>) {
    SPOT.set(None);
    SOUGHT.with_borrow_mut(|s| *s = sought.map(|(name, _)| name));
    LIT.with_borrow_mut(|lit| *lit = sought.map(|(name, color)| (name, color, Instant::now())));
}

/// The name of the row a search result opened, until the pane has scrolled to it.
pub fn sought() -> Option<&'static str> {
    SOUGHT.with_borrow(|s| *s)
}

/// Whether a found row is still fading, so the dialog keeps drawing frames until it is done.
pub fn flashing() -> bool {
    LIT.with_borrow(|lit| lit.is_some_and(|(_, _, at)| at.elapsed() < FLASH))
}

/// How lit the row named `name` is now, from 1 just found to 0 once the flash is over.
fn glow(name: &str) -> Option<Hsla> {
    LIT.with_borrow(|lit| {
        let (sought, color, at) = lit.as_ref()?;
        if !sought.eq_ignore_ascii_case(name) {
            return None;
        }
        let left = 1. - at.elapsed().as_secs_f32() / FLASH.as_secs_f32();
        // Held near full for a moment, then eased out, so the flash reads as one pulse.
        (left > 0.).then_some(Hsla {
            a: color.a * (left * (2. - left)),
            ..*color
        })
    })
}

/// `el`, which records where it is drawn while a search result names `name`, and is lit while the
/// flash for it lasts.
pub fn anchored(el: Stateful<Div>, name: &str) -> Stateful<Div> {
    let sought = SOUGHT.with_borrow(|s| s.is_some_and(|s| s.eq_ignore_ascii_case(name)));
    el.when(sought, |el| {
        el.relative().child(
            canvas(|bounds, _, _| SPOT.set(Some(bounds)), |_, (), _, _| {})
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
        )
    })
    .when_some(glow(name), gpui::Styled::bg)
}

/// Scrolls `scroll` so the sought row sits in the middle of the pane, once the row is drawn.
pub fn center_when_drawn(scroll: ScrollHandle, window: &Window) {
    wait_for_row(scroll, window, TRIES);
}

fn wait_for_row(scroll: ScrollHandle, window: &Window, tries: u8) {
    window.on_next_frame(move |window, _| {
        let Some(row) = SPOT.get() else {
            if let Some(tries) = tries.checked_sub(1) {
                window.refresh();
                wait_for_row(scroll, window, tries);
            }
            return;
        };
        let view = scroll.bounds();
        let offset = scroll.offset();
        let lowest = -f32::from(scroll.max_offset().y);
        let shift = f32::from(row.center().y) - f32::from(view.center().y);
        let y = (f32::from(offset.y) - shift).clamp(lowest.min(0.), 0.);
        scroll.set_offset(point(offset.x, px(y)));
        SOUGHT.with_borrow_mut(|s| *s = None);
        SPOT.set(None);
        window.refresh();
    });
}
