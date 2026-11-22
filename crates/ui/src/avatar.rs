//! Avatars.
//!
//! A little stitched cube like Patchwork's logo, in colours picked from an id, with a face that
//! shows a mood. Bots and the chats they run wear them. They sway, breathe and blink, and their
//! eyes follow the pointer, unless the person asked for less motion.

// Rust guideline compliant 2026-10-09

use std::f32::consts::{PI, TAU};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use crate::redraw_after;
use gpui::{
    Background, Hsla, PathBuilder, Pixels, Point, Role, Window, canvas, div, hsla,
    linear_color_stop, linear_gradient, point, prelude::*, px,
};

/// How the owner of an avatar is doing, which its eyes show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    /// Nothing to do.
    Idle,
    /// A reply is being written.
    Working,
    /// It waits on the person: an approval or a question.
    Waiting,
    /// Its last reply failed.
    Error,
}

impl Mood {
    /// The mood as a screen reader says it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Mood::Idle => "idle",
            Mood::Working => "working",
            Mood::Waiting => "waiting for you",
            Mood::Error => "something went wrong",
        }
    }
}

/// The hues a cube's top can be, in turns of the colour wheel. Its sides take hues further
/// round, as the logo's pink top sits over blue and violet sides.
const HUES: [f32; 8] = [0.92, 0.0, 0.08, 0.14, 0.33, 0.47, 0.58, 0.75];

/// How far the half width of a cube is from its centre, against its half height.
const HALF_WIDTH: f32 = 0.866;

/// How long a blink takes, in seconds.
const BLINK: f32 = 0.26;

/// How far the pointer can be, in avatar widths, before the eyes look all the way at it.
const GAZE_REACH: f32 = 6.;

/// FNV-1a of `id`, which picks the same shape and colour for it every time.
fn seed(id: &str) -> u64 {
    id.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Seconds on the clock every avatar moves by, so two avatars of one id move together.
fn clock() -> f32 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

/// How long a working avatar waits between frames. Its motion is quick but small, so a lower
/// rate than the screen's looks the same and redraws the window far less.
const FRAME: Duration = Duration::from_millis(1000 / 24);

/// How long an avatar that isn't working waits between frames while it stirs or blinks.
const CALM: Duration = Duration::from_millis(1000 / 15);

/// How long an avatar that isn't working stirs, in seconds: it sways and breathes, then rests.
const STIR: f32 = 3.;

/// Avatars this small are too small to see sway, so they keep still and only blink.
const SMALL: f32 = 24.;

/// How long until avatar `seed` next blinks at `t`; zero while it blinks.
fn until_blink(seed: u64, t: f32) -> f32 {
    let every = 3.4 + (seed % 23) as f32 / 10.;
    let at = (t + (seed % 997) as f32 / 97.) % every;
    if at < BLINK { 0. } else { every - at }
}

/// How shut the eyes are at `t`, from 0 (open) to 1 (shut). Each avatar blinks every few
/// seconds, on a beat of its own.
fn blink(seed: u64, t: f32) -> f32 {
    let every = 3.4 + (seed % 23) as f32 / 10.;
    let at = (t + (seed % 997) as f32 / 97.) % every;
    if at < BLINK {
        (PI * at / BLINK).sin().powi(2)
    } else {
        0.
    }
}

/// How far avatar `seed` is into a stir at `t`, from 0 (resting) to 1, and how long until its
/// next stir starts. Each one stirs every 10 to 14 seconds, on a beat of its own, so a page of
/// avatars mostly rests and the window is seldom redrawn.
fn stir(seed: u64, t: f32) -> (f32, f32) {
    let every = 10. + ((seed >> 16) % 41) as f32 / 10.;
    let at = (t + ((seed >> 4) % 997) as f32 / 61.) % every;
    if at < STIR {
        ((PI * at / STIR).sin().powi(2), 0.)
    } else {
        (0., every - at)
    }
}

/// How the avatar sits at `t`: how far it moves across and down, how far it leans, and how
/// much it breathes in.
struct Pose {
    dx: f32,
    dy: f32,
    lean: f32,
    breath: f32,
}

impl Pose {
    const STILL: Pose = Pose {
        dx: 0.,
        dy: 0.,
        lean: 0.,
        breath: 0.,
    };

    fn at(mood: Mood, size: f32, t: f32) -> Pose {
        let wave = |period: f32, shift: f32| (TAU * t / period + shift).sin();
        match mood {
            // A slow sway and breath.
            Mood::Idle => Pose {
                dx: size * 0.03 * wave(6., 0.),
                dy: size * 0.025 * wave(3., 0.5),
                lean: 0.05 * wave(6., 1.),
                breath: 0.025 * wave(3., 0.),
            },
            // A busy little hop.
            Mood::Working => Pose {
                dx: 0.,
                dy: -size * 0.05 * wave(0.9, 0.).abs(),
                lean: 0.04 * wave(1.8, 0.),
                breath: 0.03 * wave(0.9, 0.5),
            },
            // Deep, slow breaths.
            Mood::Waiting => Pose {
                dy: size * 0.02 * wave(4., 0.),
                breath: 0.04 * wave(4., 0.),
                ..Pose::STILL
            },
            Mood::Error => Pose {
                breath: 0.015 * wave(4., 0.),
                ..Pose::STILL
            },
        }
    }

    /// This pose with its motion scaled by `by`, from 0 (still) to 1.
    fn scaled(self, by: f32) -> Pose {
        Pose {
            dx: self.dx * by,
            dy: self.dy * by,
            lean: self.lean * by,
            breath: self.breath * by,
        }
    }
}

/// A filled oval `rx` wide and `ry` tall around `c`.
fn oval(window: &mut Window, c: Point<Pixels>, rx: f32, ry: f32, color: Hsla) {
    if ry < 0.2 {
        return;
    }
    let r = point(px(rx), px(ry));
    let mut path = PathBuilder::fill();
    path.move_to(point(c.x + r.x, c.y));
    path.arc_to(r, px(0.), false, true, point(c.x - r.x, c.y));
    path.arc_to(r, px(0.), false, true, point(c.x + r.x, c.y));
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// A line from `a` to `b`, `width` wide.
fn line(window: &mut Window, a: Point<Pixels>, b: Point<Pixels>, width: f32, color: Hsla) {
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(a);
    path.line_to(b);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// Where the eyes look, from -1 to 1 each way: at the pointer when idle, back and forth over
/// the work when working, and ahead otherwise.
fn gaze(mood: Mood, c: Point<Pixels>, pointer: Point<Pixels>, size: f32, t: f32) -> (f32, f32) {
    match mood {
        Mood::Idle => {
            let reach = size * GAZE_REACH;
            let x = f32::from(pointer.x - c.x) / reach;
            let y = f32::from(pointer.y - c.y) / reach;
            let far = (x * x + y * y).sqrt().max(1.);
            (x / far, y / far)
        }
        Mood::Working => ((TAU * t / 1.6).sin(), -0.5),
        Mood::Waiting | Mood::Error => (0., 0.),
    }
}

/// A point on the screen, as plain numbers for working out shapes.
type Spot = [f32; 2];

fn at(p: Spot) -> Point<Pixels> {
    point(px(p[0]), px(p[1]))
}

/// The point `d` along the way from `from` to `to`, going no further than halfway.
fn toward(from: Spot, to: Spot, d: f32) -> Spot {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len <= f32::EPSILON {
        return from;
    }
    let d = d.min(len / 2.) / len;
    [from[0] + dx * d, from[1] + dy * d]
}

/// `corners` drawn in a share `by` of the way towards their middle.
fn shrink<const N: usize>(corners: [Spot; N], by: f32) -> [Spot; N] {
    let n = N as f32;
    let mid = corners
        .iter()
        .fold([0., 0.], |m, c| [m[0] + c[0] / n, m[1] + c[1] / n]);
    corners.map(|c| [c[0] + (mid[0] - c[0]) * by, c[1] + (mid[1] - c[1]) * by])
}

/// A filled shape through `corners`, each corner rounded off by `round` pixels.
fn rounded(window: &mut Window, corners: &[Spot], round: f32, fill: impl Into<Background>) {
    let n = corners.len();
    let mut path = PathBuilder::fill();
    for (i, &corner) in corners.iter().enumerate() {
        let before = toward(corner, corners[(i + n - 1) % n], round);
        let after = toward(corner, corners[(i + 1) % n], round);
        if i == 0 {
            path.move_to(at(before));
        } else {
            path.line_to(at(before));
        }
        path.curve_to(at(after), at(corner));
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, fill);
    }
}

/// Stitches along `points`, `width` wide.
fn stitches(window: &mut Window, points: &[Spot], width: f32, color: Hsla) {
    let mut path = PathBuilder::stroke(px(width)).dash_array(&[px(width * 2.), px(width * 1.6)]);
    for (i, &p) in points.iter().enumerate() {
        if i == 0 {
            path.move_to(at(p));
        } else {
            path.line_to(at(p));
        }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// A mouth from `left` to `right`, bowed down by `bow` (up when below zero).
fn mouth(window: &mut Window, left: Spot, right: Spot, bow: f32, width: f32, color: Hsla) {
    let mid = [
        f32::midpoint(left[0], right[0]),
        f32::midpoint(left[1], right[1]) + bow * 2.,
    ];
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(at(left));
    path.curve_to(at(right), at(mid));
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// The colour `h` turns of the wheel round, at saturation `s` and lightness `l`.
fn tone(h: f32, s: f32, l: f32) -> Hsla {
    hsla(h.rem_euclid(1.), s, l, 1.)
}

/// The avatar of `id`, `size` pixels square, with a face for `mood`. Screen readers read it
/// as `name` and the mood.
#[must_use]
pub fn avatar(id: &str, name: &str, mood: Mood, size: f32) -> impl IntoElement {
    let seed = seed(id);
    let top = HUES[usize::try_from((seed >> 8) % HUES.len() as u64).unwrap_or(0)];
    let (left, right) = (top - 0.37, top - 0.17);
    let top_fill = linear_gradient(
        180.,
        linear_color_stop(tone(top, 0.9, 0.97), 0.),
        linear_color_stop(tone(top, 0.8, 0.85), 1.),
    );
    let left_fill = linear_gradient(
        150.,
        linear_color_stop(tone(left, 0.8, 0.93), 0.),
        linear_color_stop(tone(left, 0.6, 0.76), 1.),
    );
    let right_fill = linear_gradient(
        210.,
        linear_color_stop(tone(right, 0.7, 0.85), 0.),
        linear_color_stop(tone(right, 0.55, 0.64), 1.),
    );
    let rim = tone(right, 0.4, 0.42);
    let ink = tone(right, 0.45, 0.17);
    let thread = hsla(right.rem_euclid(1.), 0.45, 0.3, 0.75);
    let cheek = hsla(0.95, 0.9, 0.72, 0.55);
    let shine = hsla(0., 0., 1., 1.);
    // Each avatar is at its own point in the sway, so a list of them doesn't move as one.
    let shift = ((seed >> 24) % 600) as f32 / 100.;
    let drawn = canvas(
        |_, _, _| {},
        move |bounds, (), window: &mut Window, cx| {
            // A window in the background keeps its avatars still: it is drawn again as it comes
            // back to the front.
            let still = cx.reduce_motion() || !window.is_window_active();
            let now = clock() + shift;
            let small = size < SMALL;
            let (pose, shut) = if still {
                (Pose::STILL, 0.)
            } else if mood == Mood::Working {
                redraw_after(FRAME, window, cx);
                (Pose::at(mood, size, now), blink(seed, now))
            } else {
                // It rests between blinks and stirs, and wakes only for those.
                let (stirring, until_stir) = if small {
                    (0., f32::MAX)
                } else {
                    stir(seed, now)
                };
                let shut = blink(seed, now);
                if stirring > 0. || shut > 0. {
                    redraw_after(CALM, window, cx);
                } else {
                    let wait = until_blink(seed, now).min(until_stir);
                    redraw_after(Duration::from_secs_f32(wait).max(CALM), window, cx);
                }
                (Pose::at(mood, size, now).scaled(stirring), shut)
            };
            let mid = bounds.center();
            let (cx0, cy0) = (f32::from(mid.x) + pose.dx, f32::from(mid.y) + pose.dy);
            // Breathing in makes it a little taller and narrower.
            let (sx, sy) = (1. - pose.breath * 0.5, 1. + pose.breath);
            // Leave room for the sway.
            let full = size / 2. * if small { 0.92 } else { 0.8 };
            let (sin, cos) = pose.lean.sin_cos();
            // A point of the cube, from -1 to 1 each way, placed on the screen.
            let put = |x: f32, y: f32| -> Spot {
                let (x, y) = (x * full * sx, y * full * sy);
                [cx0 + x * cos - y * sin, cy0 + x * sin + y * cos]
            };
            let half = HALF_WIDTH;
            let top_point = put(0., -1.);
            let (top_left, top_right) = (put(-half, -0.5), put(half, -0.5));
            let middle = put(0., 0.);
            let (low_left, low_right) = (put(-half, 0.5), put(half, 0.5));
            let bottom = put(0., 1.);
            let top_face = [top_point, top_right, middle, top_left];
            let round = full * 0.16;
            rounded(
                window,
                &[top_point, top_right, low_right, bottom, low_left, top_left],
                round,
                rim,
            );
            let inset = if small { 0.06 } else { 0.035 };
            rounded(window, &shrink(top_face, inset), round * 0.7, top_fill);
            let left_face = [top_left, middle, bottom, low_left];
            rounded(window, &shrink(left_face, inset), round * 0.7, left_fill);
            let right_face = [middle, top_right, low_right, bottom];
            rounded(window, &shrink(right_face, inset), round * 0.7, right_fill);
            let stroke = (size * 0.022).max(1.);
            if !small {
                let seam = shrink(top_face, 0.16);
                stitches(window, &[seam[3], seam[2], seam[1]], stroke, thread);
                stitches(window, &[put(0., 0.66), put(0., 0.9)], stroke, thread);
            }
            let (lx, ly) = if still {
                (0., 0.)
            } else {
                gaze(mood, mid, window.mouse_position(), size, now)
            };
            let eye_r = full * 0.14;
            let open = 1. - shut;
            for side in [-1., 1.] {
                let eye = put(side * 0.34, 0.24);
                if !small {
                    let blush = put(side * 0.56, 0.5);
                    oval(window, at(blush), eye_r * 0.8, eye_r * 0.45, cheek);
                }
                match mood {
                    Mood::Idle | Mood::Working => {
                        let look = eye_r * 0.35;
                        let e = [eye[0] + lx * look, eye[1] + ly * look * open];
                        oval(window, at(e), eye_r * 0.78, eye_r * open, ink);
                        if open > 0.6 && size >= 14. {
                            let r = eye_r * 0.32;
                            let glint = point(px(e[0] - r * 0.8), px(e[1] - r * 1.1));
                            oval(window, glint, r, r, shine);
                        }
                    }
                    Mood::Waiting => {
                        // Half shut, looking out.
                        let w = eye_r * 0.85;
                        line(
                            window,
                            at([eye[0] - w, eye[1]]),
                            at([eye[0] + w, eye[1]]),
                            (size * 0.06).max(1.5),
                            ink,
                        );
                    }
                    Mood::Error => {
                        let w = eye_r * 0.65;
                        for flip in [-1., 1.] {
                            line(
                                window,
                                at([eye[0] - w, eye[1] - flip * w]),
                                at([eye[0] + w, eye[1] + flip * w]),
                                (size * 0.05).max(1.2),
                                ink,
                            );
                        }
                    }
                }
            }
            if !small {
                let (l, r) = (put(-0.1, 0.48), put(0.1, 0.48));
                match mood {
                    Mood::Idle => mouth(window, l, r, full * 0.05, stroke * 1.3, ink),
                    Mood::Working => {
                        let m = put(0., 0.5);
                        oval(window, at(m), full * 0.06, full * 0.075, ink);
                    }
                    Mood::Waiting => mouth(window, l, r, 0., stroke * 1.3, ink),
                    Mood::Error => mouth(window, l, r, -full * 0.04, stroke * 1.3, ink),
                }
            }
        },
    )
    .size_full();
    div()
        .id(gpui::ElementId::Name(format!("avatar:{id}").into()))
        .role(Role::Image)
        .aria_label(format!("{name}, {}", mood.label()))
        .flex_none()
        .size(px(size))
        .child(drawn)
}
