//! The scrollbar every scrolling view shares: a thumb that follows the content, and that the
//! pointer drags or jumps by pressing the track.

// Rust guideline compliant 2026-10-09

use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, Div, Global, HitboxBehavior, Hsla, ListState,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Role, ScrollHandle,
    Stateful, canvas, div, fill, point, prelude::*, px, size,
};

/// The narrowest strip the pointer can press, however thin the thumb is drawn.
const HIT_WIDTH: f32 = 10.;
/// The shortest the thumb gets, however long the content.
const THUMB_MIN: f32 = 20.;

/// What a scrollbar scrolls.
#[derive(Clone)]
enum Target {
    Handle(ScrollHandle),
    List(ListState),
}

impl Target {
    /// How far the content can scroll.
    fn max(&self) -> f32 {
        match self {
            Self::Handle(handle) => f32::from(handle.max_offset().y),
            Self::List(list) => f32::from(list.max_offset_for_scrollbar().y),
        }
    }

    /// How far it has scrolled.
    fn scrolled(&self) -> f32 {
        match self {
            Self::Handle(handle) => -f32::from(handle.offset().y),
            Self::List(list) => -f32::from(list.scroll_px_offset_for_scrollbar().y),
        }
    }

    fn scroll_to(&self, scrolled: f32) {
        match self {
            Self::Handle(handle) => handle.set_offset(point(handle.offset().x, px(-scrolled))),
            Self::List(list) => list.set_offset_from_scrollbar(point(px(0.), px(-scrolled))),
        }
    }

    /// A list holds its measured height still for the length of a drag.
    fn drag_started(&self) {
        if let Self::List(list) = self {
            list.scrollbar_drag_started();
        }
    }

    fn drag_ended(&self) {
        if let Self::List(list) = self {
            list.scrollbar_drag_ended();
        }
    }
}

/// The scrollbar the pointer holds, by its track, and how far down the thumb it took hold.
#[derive(Default)]
struct Drag(Option<(Bounds<Pixels>, f32)>);

impl Global for Drag {}

/// A scrollbar for the element `handle` tracks, its thumb `width` wide with `inset` of clear
/// space around it. None when nothing scrolls.
pub fn scrollbar(
    handle: &ScrollHandle,
    width: f32,
    inset: f32,
    color: Hsla,
) -> Option<Stateful<Div>> {
    bar(Target::Handle(handle.clone()), width, inset, color)
}

/// `scrollbar` for a `list`, sized from the rows it has measured so far.
pub fn list_scrollbar(
    state: &ListState,
    width: f32,
    inset: f32,
    color: Hsla,
) -> Option<Stateful<Div>> {
    bar(Target::List(state.clone()), width, inset, color)
}

fn bar(target: Target, width: f32, inset: f32, color: Hsla) -> Option<Stateful<Div>> {
    let max = target.max();
    if max <= 0. {
        return None;
    }
    let value = target.scrolled().clamp(0., max);
    let track = canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::BlockMouse),
        move |bounds, hitbox, window, cx| {
            paint(&target, bounds, &hitbox, width, inset, color, window, cx);
        },
    )
    .size_full();
    Some(
        div()
            .id("scrollbar")
            .role(Role::ScrollBar)
            .aria_label("Scroll")
            .aria_orientation(gpui::Orientation::Vertical)
            .aria_numeric_value(f64::from(value))
            .aria_min_numeric_value(0.)
            .aria_max_numeric_value(f64::from(max))
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(width.max(HIT_WIDTH)))
            .child(track),
    )
}

/// Draws the thumb in its track, `bounds`, and lets the pointer move it: a press on the thumb
/// takes hold of it there, and a press on the track brings the thumb's middle under the pointer.
#[expect(
    clippy::too_many_arguments,
    reason = "one call, from the track's canvas"
)]
fn paint(
    target: &Target,
    bounds: Bounds<Pixels>,
    hitbox: &gpui::Hitbox,
    width: f32,
    inset: f32,
    color: Hsla,
    window: &mut gpui::Window,
    cx: &mut App,
) {
    let view = f32::from(bounds.size.height);
    let max = target.max();
    if max <= 0. || view <= 0. {
        return;
    }
    let thumb = (view * view / (view + max)).max(THUMB_MIN).min(view);
    let travel = view - thumb;
    let top = target.scrolled().clamp(0., max) / max * travel;
    let held = cx
        .default_global::<Drag>()
        .0
        .is_some_and(|(track, _)| track == bounds);
    let shown = if held || hitbox.is_hovered(window) {
        color.opacity((color.a * 1.6).min(1.))
    } else {
        color
    };
    window.paint_quad(
        fill(
            Bounds::new(
                point(
                    bounds.right() - px(width - inset),
                    bounds.top() + px(top + inset),
                ),
                size(px(width - inset * 2.), px(thumb - inset * 2.)),
            ),
            shown,
        )
        .corner_radii(px(width / 2.)),
    );
    window.set_cursor_style(CursorStyle::Arrow, hitbox);
    if travel <= 0. {
        return;
    }
    // Where the content sits when the thumb's top is `y` below the track's.
    let at = move |y: f32| (y / travel).clamp(0., 1.) * max;

    let (pressed, over) = (target.clone(), hitbox.clone());
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble
            || event.button != MouseButton::Left
            || !over.is_hovered(window)
        {
            return;
        }
        let y = f32::from(event.position.y - bounds.top());
        let grip = if (top..top + thumb).contains(&y) {
            y - top
        } else {
            thumb / 2.
        };
        pressed.drag_started();
        pressed.scroll_to(at(y - grip));
        cx.default_global::<Drag>().0 = Some((bounds, grip));
        cx.stop_propagation();
        window.refresh();
    });
    let moved = target.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture {
            return;
        }
        let Some((track, grip)) = cx.default_global::<Drag>().0 else {
            return;
        };
        if track != bounds {
            return;
        }
        if event.pressed_button == Some(MouseButton::Left) {
            moved.scroll_to(at(f32::from(event.position.y - bounds.top()) - grip));
        } else {
            // The button came up where the window could not see it.
            cx.default_global::<Drag>().0 = None;
            moved.drag_ended();
        }
        window.refresh();
    });
    let released = target.clone();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
        let drag = cx.default_global::<Drag>();
        if phase == DispatchPhase::Capture
            && event.button == MouseButton::Left
            && drag.0.is_some_and(|(track, _)| track == bounds)
        {
            drag.0 = None;
            released.drag_ended();
            window.refresh();
        }
    });
}
