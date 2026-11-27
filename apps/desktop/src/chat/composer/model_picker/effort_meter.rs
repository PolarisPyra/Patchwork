//! How hard the model thinks, as a slider at the foot of the model picker: Auto at the left, then
//! each level the model takes. The knob follows a drag freely and settles on the nearest level
//! when let go; the arrows step it while it has the keys, and Shift with an arrow steps it from
//! the search field.
//!
//! The fill warms with the level, from a cool cyan just past Auto through green and amber to a
//! red-orange at the top, as the web composer's `EffortMeter`.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, Bounds, BoxShadow, DispatchPhase, FontWeight, Hitbox, HitboxBehavior, Hsla,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Role,
    WeakEntity, Window, canvas, div, fill, hsla, linear_color_stop, linear_gradient, point,
    prelude::*, px, size,
};
use patchwork_backend::provider::AgentKind;
use patchwork_ui_markdown::effort_label;
use patchwork_ui_components::focus_ring;
use patchwork_ui_theme::{Theme, fs_xs};

use crate::chat::composer::Composer;
use crate::chat::composer::menus::Pick;

/// How far in from the track's ends the first and last stops sit, so their dots stay inside it.
const INSET: f32 = 6.;
/// The track's height.
const TRACK: f32 = 8.;
/// The knob's width, grown a tenth while it is held.
const KNOB: f32 = 16.;
/// The dot at each stop.
const DOT: f32 = 4.;
/// The coloured core of the knob.
const CORE: f32 = 6.;

/// The name of an effort, with no level picked shown as "Auto".
#[must_use]
pub fn effort_name(effort: Option<&str>) -> String {
    effort.map_or_else(|| "Auto".into(), effort_label)
}

/// The stops a model's effort can sit at, Auto first, and the one it is at.
fn stops_of(levels: &[String], effort: Option<&str>) -> (Vec<Option<String>>, usize) {
    let stops: Vec<Option<String>> = std::iter::once(None)
        .chain(levels.iter().cloned().map(Some))
        .collect();
    let at = stops
        .iter()
        .position(|s| s.as_deref() == effort)
        .unwrap_or(0);
    (stops, at)
}

/// A colour on the meter's scale, `hue` in degrees.
fn hsl(hue: f32, saturation: f32, lightness: f32) -> Hsla {
    hsla(hue.rem_euclid(360.) / 360., saturation, lightness, 1.)
}

/// The hue of the level `heat` of the way along: 195 (cyan) at Auto down to 10 (red-orange).
fn hue_at(heat: f32) -> f32 {
    195. - heat * 185.
}

impl Composer {
    /// The agent, model and effort the open picker is for: the Orchestrator page's while it is
    /// that page's, else the chat's or the next chat's.
    pub(super) fn picked(
        &self,
        cx: &gpui::App,
    ) -> Option<(AgentKind, Option<String>, Option<String>)> {
        let workspace = self.workspace.upgrade()?;
        let w = workspace.read(cx);
        if self.launching {
            let (agent, model) = self.launch_pick(&w.settings);
            return Some((agent, model, self.launch_effort(&w.settings)));
        }
        let picks = self.picks(w);
        Some((picks.agent, picks.model, picks.effort))
    }

    /// Moves the effort `by` stops from where it is, as Shift with an arrow does in the search
    /// field, staying at the ends rather than going round.
    pub(super) fn nudge_effort(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some((agent, model, effort)) = self.picked(cx) else {
            return;
        };
        let levels = agent.model_efforts(model.as_deref(), self.models_of(agent));
        if levels.is_empty() {
            return;
        }
        let (stops, at) = stops_of(&levels, effort.as_deref());
        let next = at.saturating_add_signed(by).min(stops.len() - 1);
        if next != at
            && let Some(stop) = stops.get(next)
        {
            self.commit(Pick::Effort(stop.clone()), cx);
        }
    }

    /// The meter for `levels`, at `effort`; None when the model has no levels to pick from.
    pub(super) fn effort_meter(
        &mut self,
        levels: &[String],
        effort: Option<&str>,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if levels.is_empty() {
            return None;
        }
        let focus = self.picker.as_ref()?.effort.clone();
        let (stops, chosen) = stops_of(levels, effort);
        let last = stops.len() - 1;
        // A drag shows where the pointer is; settled, the knob sits on the chosen stop.
        let heat = self
            .effort_drag
            .unwrap_or(chosen as f32 / last as f32)
            .clamp(0., 1.);
        let at = self
            .effort_drag
            .map_or(chosen, |share| (share * last as f32).round() as usize);
        let name = effort_name(stops.get(at).cloned().flatten().as_deref());
        let held = self.effort_drag.is_some();
        let inks = Inks {
            track: t.bg_hover,
            line: t.border_strong,
        };
        let composer = cx.entity().downgrade();
        let shape = Shape {
            heat,
            at,
            last,
            held,
        };
        let picks = stops.clone();
        let slider = canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::BlockMouse),
            move |bounds, hitbox, window, _| {
                paint(bounds, shape, inks, window);
                listen(bounds, &hitbox, last, &composer, &picks, window);
            },
        )
        .size_full();
        let keyed = stops.clone();
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .px(px(14.))
                .pt(px(8.))
                .pb(px(10.))
                .border_t_1()
                .border_dashed()
                .border_color(t.border_strong)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(fs_xs()))
                        .child(div().text_color(t.text_subtle).child("Effort"))
                        .child(
                            div()
                                .text_color(t.text)
                                .font_weight(FontWeight::NORMAL)
                                .child(name.clone()),
                        ),
                )
                .child(
                    div()
                        .id("effort-slider")
                        .role(Role::Slider)
                        .aria_label("Effort")
                        .aria_numeric_value(at as f64)
                        .aria_min_numeric_value(0.)
                        .aria_max_numeric_value(last as f64)
                        .aria_value(name)
                        .track_focus(&focus)
                        .focus_visible(focus_ring)
                        .rounded(px(4.))
                        .h(px(20.))
                        .w_full()
                        .cursor_pointer()
                        .on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| {
                            let next = match e.keystroke.key.as_str() {
                                "left" | "down" => chosen.saturating_sub(1),
                                "right" | "up" => (chosen + 1).min(last),
                                "home" => 0,
                                "end" => last,
                                _ => return,
                            };
                            cx.stop_propagation();
                            if next != chosen
                                && let Some(stop) = keyed.get(next)
                            {
                                this.commit(Pick::Effort(stop.clone()), cx);
                            }
                        }))
                        .child(slider),
                )
                .into_any_element(),
        )
    }
}

/// Where the meter is: how far along the knob is, the stop nearest it, the last stop, and whether
/// the pointer holds it.
#[derive(Clone, Copy)]
struct Shape {
    heat: f32,
    at: usize,
    last: usize,
    held: bool,
}

/// The theme's colours the meter is painted in.
#[derive(Clone, Copy)]
struct Inks {
    track: Hsla,
    line: Hsla,
}

/// Paints the track, its fill up to the knob, a dot at each stop, and the knob.
fn paint(bounds: Bounds<Pixels>, shape: Shape, inks: Inks, window: &mut Window) {
    let width = f32::from(bounds.size.width);
    let travel = width - INSET * 2.;
    if travel <= 0. {
        return;
    }
    let middle = bounds.top() + bounds.size.height / 2.;
    let left = bounds.left();
    let track = Bounds::new(
        point(left, middle - px(TRACK / 2.)),
        size(px(width), px(TRACK)),
    );
    window.paint_quad(fill(track, inks.track).corner_radii(px(TRACK / 2.)));

    let knob_x = INSET + travel * shape.heat;
    let hue = hue_at(shape.heat);
    // The fill fades in over the first sliver past Auto, so Auto itself shows a bare track.
    let opacity = (shape.heat * 12.).min(1.);
    if opacity > 0. {
        // The web's gradient runs the whole track and is cut at the knob; this one is drawn to
        // the knob with its far end at the colour the full gradient has there.
        let reach = knob_x / width;
        let from = hsl(hue + 35., 0.8, 0.58).opacity(opacity);
        let to = hsl(hue + 35. - 45. * reach, 0.8 + 0.07 * reach, 0.58).opacity(opacity);
        let filled = Bounds::new(track.origin, size(px(knob_x), px(TRACK)));
        window.paint_quad(
            fill(
                filled,
                linear_gradient(90., linear_color_stop(from, 0.), linear_color_stop(to, 1.)),
            )
            .corner_radii(px(TRACK / 2.)),
        );
    }

    let lit = gpui::white().opacity(0.55);
    for i in 0..=shape.last {
        let x = INSET + travel * i as f32 / shape.last.max(1) as f32;
        let dot = Bounds::new(
            point(left + px(x - DOT / 2.), middle - px(DOT / 2.)),
            size(px(DOT), px(DOT)),
        );
        let color = if i < shape.at { lit } else { inks.line };
        window.paint_quad(fill(dot, color).corner_radii(px(DOT / 2.)));
    }

    let knob = if shape.held { KNOB * 1.1 } else { KNOB };
    let knob_bounds = Bounds::new(
        point(left + px(knob_x - knob / 2.), middle - px(knob / 2.)),
        size(px(knob), px(knob)),
    );
    window.paint_drop_shadows(
        knob_bounds,
        gpui::Corners::all(px(knob / 2.)),
        &[BoxShadow {
            color: gpui::black().opacity(0.45),
            offset: point(px(0.), px(1.)),
            blur_radius: px(3.),
            spread_radius: px(0.),
            inset: false,
        }],
    );
    window.paint_quad(fill(knob_bounds, gpui::white()).corner_radii(px(knob / 2.)));
    let core = if shape.at > 0 {
        hsl(hue, 0.85, 0.5)
    } else {
        inks.line
    };
    window.paint_quad(
        fill(
            Bounds::new(
                point(left + px(knob_x - CORE / 2.), middle - px(CORE / 2.)),
                size(px(CORE), px(CORE)),
            ),
            core,
        )
        .corner_radii(px(CORE / 2.)),
    );
}

/// Lets the pointer move the knob: a press takes it to the pointer and holds it, it follows the
/// pointer while held, and letting go picks the nearest stop.
fn listen(
    bounds: Bounds<Pixels>,
    hitbox: &Hitbox,
    last: usize,
    composer: &WeakEntity<Composer>,
    stops: &[Option<String>],
    window: &mut Window,
) {
    let travel = f32::from(bounds.size.width) - INSET * 2.;
    if travel <= 0. {
        return;
    }
    let share = move |x: Pixels| (f32::from(x - bounds.left() - px(INSET)) / travel).clamp(0., 1.);
    let over = hitbox.clone();
    let pressed = composer.clone();
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble
            || event.button != MouseButton::Left
            || !over.is_hovered(window)
        {
            return;
        }
        let _ = pressed.update(cx, |this, cx| {
            this.effort_drag = Some(share(event.position.x));
            // The slider takes the keys, as a press on the web's does.
            if let Some(picker) = &this.picker {
                window.focus(&picker.effort, cx);
            }
            cx.notify();
        });
        cx.stop_propagation();
    });
    let moved = composer.clone();
    let settle = {
        let stops = stops.to_vec();
        move |this: &mut Composer, x: Pixels, cx: &mut Context<Composer>| {
            this.effort_drag = None;
            let stop = (share(x) * last as f32).round() as usize;
            if let Some(stop) = stops.get(stop) {
                this.commit(Pick::Effort(stop.clone()), cx);
            }
            cx.notify();
        }
    };
    let released = settle.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
        if phase != DispatchPhase::Capture {
            return;
        }
        let _ = moved.update(cx, |this, cx| {
            if this.effort_drag.is_none() {
                return;
            }
            if event.pressed_button == Some(MouseButton::Left) {
                this.effort_drag = Some(share(event.position.x));
                cx.notify();
            } else {
                // The button came up where the window could not see it.
                settle(this, event.position.x, cx);
            }
        });
    });
    let up = composer.clone();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
            return;
        }
        let _ = up.update(cx, |this, cx| {
            if this.effort_drag.is_some() {
                released(this, event.position.x, cx);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_is_the_first_stop_and_an_unknown_level_falls_back_to_it() {
        let levels = vec!["low".to_string(), "high".to_string()];
        let (stops, at) = stops_of(&levels, Some("high"));
        assert_eq!(stops.len(), 3);
        assert_eq!(at, 2);
        assert_eq!(stops_of(&levels, None).1, 0);
        assert_eq!(stops_of(&levels, Some("max")).1, 0);
    }

    #[test]
    fn the_hue_runs_from_cyan_to_red_orange() {
        assert!((hue_at(0.) - 195.).abs() < f32::EPSILON);
        assert!((hue_at(1.) - 10.).abs() < f32::EPSILON);
        assert_eq!(effort_name(None), "Auto");
    }
}
