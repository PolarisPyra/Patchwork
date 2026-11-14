//! A slider of the theme settings: a knob on a track, moved by the pointer or the arrow keys,
//! that sets its value while it moves.

// Rust guideline compliant 2026-10-09

use std::rc::Rc;

use gpui::{
    App, Bounds, DispatchPhase, Div, Global, HitboxBehavior, Hsla, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Role, SharedString, Stateful, WeakEntity,
    canvas, div, fill, point, prelude::*, px, size,
};

use crate::settings::SettingsDialog;
use crate::ui::focus_ring;
use crate::ui::theme::{fs_sm, Theme as Colors};

const TRACK_W: f32 = 160.;
const TRACK_H: f32 = 20.;
const KNOB: f32 = 12.;

/// The track whose knob the pointer holds.
#[derive(Default)]
struct Held(Option<Bounds<Pixels>>);

impl Global for Held {}

/// What a slider sets: the dialog it is in, and the change a value makes there.
type Set = Rc<dyn Fn(&mut SettingsDialog, f32, &mut Context<SettingsDialog>)>;

/// What a slider goes between, and by how much an arrow key moves it.
#[derive(Clone, Copy)]
pub(super) struct Range {
    pub(super) min: f32,
    pub(super) max: f32,
    pub(super) step: f32,
}

impl Range {
    /// The value `share` of the way along, on a step.
    fn at(self, share: f32) -> f32 {
        let value = self.min + share.clamp(0., 1.) * (self.max - self.min);
        ((value / self.step).round() * self.step).clamp(self.min, self.max)
    }
}

/// A slider named `label` showing `value`, with the value written beside it.
pub(super) fn slider(
    key: impl Into<SharedString>,
    label: impl Into<SharedString>,
    value: f32,
    range: Range,
    t: &Colors,
    set: impl Fn(&mut SettingsDialog, f32, &mut Context<SettingsDialog>) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let set: Set = Rc::new(set);
    let dialog = cx.entity().downgrade();
    let (rail, fill_color, knob) = (t.border_strong, t.accent, t.text);
    let dragged = Rc::clone(&set);
    let track = canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::BlockMouse),
        move |bounds, hitbox, window, _| {
            let inks = [rail, fill_color, knob];
            paint(
                bounds,
                &hitbox,
                (value, range),
                inks,
                &dialog,
                &dragged,
                window,
            );
        },
    )
    .w(px(TRACK_W))
    .h(px(TRACK_H));
    div()
        .id(key.into())
        .role(Role::Slider)
        .aria_label(label.into())
        .aria_numeric_value(f64::from(value))
        .aria_min_numeric_value(f64::from(range.min))
        .aria_max_numeric_value(f64::from(range.max))
        .tab_index(0)
        .focus_visible(focus_ring)
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(4.))
        .rounded(px(crate::ui::theme::radius_sm()))
        .on_key_down(cx.listener(move |d, e: &KeyDownEvent, _, cx| {
            let next = match e.keystroke.key.as_str() {
                "left" | "down" => value - range.step,
                "right" | "up" => value + range.step,
                "home" => range.min,
                "end" => range.max,
                _ => return,
            };
            cx.stop_propagation();
            set(d, next.clamp(range.min, range.max), cx);
        }))
        .child(track)
        .child(
            div()
                .w(px(28.))
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .child(format!("{}", value.round())),
        )
}

/// Draws the track in `bounds` with its knob at `value`, and lets the pointer move the knob.
fn paint(
    bounds: Bounds<Pixels>,
    hitbox: &gpui::Hitbox,
    (value, range): (f32, Range),
    [rail, filled, knob]: [Hsla; 3],
    dialog: &WeakEntity<SettingsDialog>,
    set: &Set,
    window: &mut gpui::Window,
) {
    let travel = f32::from(bounds.size.width) - KNOB;
    if travel <= 0. || range.max <= range.min {
        return;
    }
    let share = ((value - range.min) / (range.max - range.min)).clamp(0., 1.);
    let middle = bounds.top() + bounds.size.height / 2.;
    let line = |width: f32, color: Hsla| {
        fill(
            Bounds::new(
                point(bounds.left() + px(KNOB / 2.), middle - px(2.)),
                size(px(width), px(4.)),
            ),
            color,
        )
        .corner_radii(px(2.))
    };
    window.paint_quad(line(travel, rail));
    window.paint_quad(line(travel * share, filled));
    window.paint_quad(
        fill(
            Bounds::new(
                point(bounds.left() + px(travel * share), middle - px(KNOB / 2.)),
                size(px(KNOB), px(KNOB)),
            ),
            knob,
        )
        .corner_radii(px(KNOB / 2.)),
    );
    // The value under the pointer at `x`, given to the dialog.
    let put = {
        let (dialog, set) = (dialog.clone(), Rc::clone(set));
        move |x: Pixels, cx: &mut App| {
            let along = f32::from(x - bounds.left()) - KNOB / 2.;
            let next = range.at(along / travel);
            let _ = dialog.update(cx, |d, cx| set(d, next, cx));
        }
    };
    let (pressed, over) = (put.clone(), hitbox.clone());
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble
            || event.button != MouseButton::Left
            || !over.is_hovered(window)
        {
            return;
        }
        cx.default_global::<Held>().0 = Some(bounds);
        pressed(event.position.x, cx);
        cx.stop_propagation();
    });
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
        if phase != DispatchPhase::Capture || cx.default_global::<Held>().0 != Some(bounds) {
            return;
        }
        if event.pressed_button == Some(MouseButton::Left) {
            put(event.position.x, cx);
        } else {
            // The button came up where the window could not see it.
            cx.default_global::<Held>().0 = None;
        }
    });
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        let held = cx.default_global::<Held>();
        if phase == DispatchPhase::Capture
            && event.button == MouseButton::Left
            && held.0 == Some(bounds)
        {
            held.0 = None;
        }
    });
}
