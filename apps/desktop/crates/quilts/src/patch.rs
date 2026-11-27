//! Patch, the Quilts page's mascot: a quilt square of four printed fabrics with a cream face
//! and the agents' avatars' glossy eyes and blush. It waves and hops when pressed, works its arm while a new tile is pointed
//! at, hops when a quilt is pinned, and frowns when nothing matches the search.
//!
//! It is drawn in its own 160 by 166 units, as the Quilts Quilt quilt draws it, and only
//! again while it acts, so at rest it costs nothing. Nearly every shape is a rounded rectangle,
//! which the GPU draws as it is; only the striped and diamond prints are paths. It keeps still
//! for less motion.

// Rust guideline compliant 2026-10-09

use std::time::{Duration, Instant};

use gpui::{
    Bounds, ContentMask, Hsla, PathBuilder, Pixels, Point, Window, canvas, fill, point, prelude::*,
    px, rgb, size,
};

use patchwork_ui_components::redraw_after;

/// A point in Patch's units.
type Spot = [f32; 2];

/// What Patch is doing for a moment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Act {
    /// Waves its arm and hops, when pressed.
    Wave,
    /// Works its arm up and down, while a new tile is pointed at.
    Sew,
    /// Jumps, when a quilt is pinned.
    Hop,
}

impl Act {
    /// How long the act lasts, in seconds: two waves, three stitches, or one hop.
    fn length(self) -> f32 {
        match self {
            Act::Wave => 1.6,
            Act::Sew => 1.2,
            Act::Hop => HOP,
        }
    }
}

/// How long a hop takes, in seconds.
const HOP: f32 = 0.45;

/// How long Patch waits between frames while it moves: 60 a second, as the shared motion clock steps.
const FRAME: Duration = Duration::from_millis(1000 / 60);

/// Patch's drawing, in its own units.
const WIDTH: f32 = 160.;
const HEIGHT: f32 = 166.;

/// The body's left, top, right and bottom, in Patch's units, and how far its corners round.
const BODY: [f32; 4] = [30., 38., 130., 140.];
const BODY_ROUND: f32 = 34.;

/// Where the seams cross, splitting the body into its four fabrics.
const SEAM: Spot = [80., 89.];

/// The value at `p`, from 0 to 1, of a motion through `keys`, each a share of the way through
/// and a value, eased between them.
fn keyed(p: f32, keys: &[(f32, f32)]) -> f32 {
    for pair in keys.windows(2) {
        let ((p0, v0), (p1, v1)) = (pair[0], pair[1]);
        if p <= p1 {
            let q = ((p - p0) / (p1 - p0)).clamp(0., 1.);
            let eased = q * q * (3. - 2. * q);
            return v0 + (v1 - v0) * eased;
        }
    }
    keys.last().map_or(0., |k| k.1)
}

/// `p` turned `degrees` clockwise round `pivot`.
fn turn(p: Spot, pivot: Spot, degrees: f32) -> Spot {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (x, y) = (p[0] - pivot[0], p[1] - pivot[1]);
    [pivot[0] + x * cos - y * sin, pivot[1] + x * sin + y * cos]
}

/// Whether `p` is inside the body drawn `inset` units smaller.
fn in_body(p: Spot, inset: f32) -> bool {
    let [x0, y0, x1, y1] = [
        BODY[0] + inset,
        BODY[1] + inset,
        BODY[2] - inset,
        BODY[3] - inset,
    ];
    if p[0] < x0 || p[0] > x1 || p[1] < y0 || p[1] > y1 {
        return false;
    }
    let r = (BODY_ROUND - inset).max(0.);
    let (dx, dy) = (
        p[0] - p[0].clamp(x0 + r, x1 - r),
        p[1] - p[1].clamp(y0 + r, y1 - r),
    );
    dx * dx + dy * dy <= r * r
}

/// The part of the line from `a` to `b` inside the body drawn `inset` units smaller. The body is
/// convex, so that part is one piece.
fn clip_to_body(a: Spot, b: Spot, inset: f32) -> Option<(Spot, Spot)> {
    const STEPS: u16 = 160;
    let along = |i: u16| {
        let q = f32::from(i) / f32::from(STEPS);
        [a[0] + (b[0] - a[0]) * q, a[1] + (b[1] - a[1]) * q]
    };
    let first = (0..=STEPS).find(|&i| in_body(along(i), inset))?;
    let last = (0..=STEPS).rev().find(|&i| in_body(along(i), inset))?;
    Some((along(first), along(last)))
}

/// A colour from its hex code.
fn hex(code: u32) -> Hsla {
    rgb(code).into()
}

/// Draws in Patch's units, moved and scaled onto the screen.
struct Pen {
    left: f32,
    top: f32,
    scale: f32,
}

impl Pen {
    fn at(&self, p: Spot) -> Point<Pixels> {
        point(
            px(self.left + p[0] * self.scale),
            px(self.top + p[1] * self.scale),
        )
    }

    fn len(&self, d: f32) -> Pixels {
        px(d * self.scale)
    }

    /// A rectangle at `x`, `y`, `w` by `h`, its corners rounded by `round`.
    fn rect(&self, window: &mut Window, [x, y, w, h]: [f32; 4], round: f32, color: Hsla) {
        let bounds = Bounds::new(self.at([x, y]), size(self.len(w), self.len(h)));
        window.paint_quad(fill(bounds, color).corner_radii(self.len(round)));
    }

    /// A pill round `c`, `rx` across and `ry` down each way: a circle when they match.
    fn pill(&self, window: &mut Window, c: Spot, rx: f32, ry: f32, color: Hsla) {
        let rect = [c[0] - rx, c[1] - ry, 2. * rx, 2. * ry];
        self.rect(window, rect, rx.min(ry), color);
    }

    /// A line through `points`, `width` wide with round ends, drawn as circles close enough
    /// together that their edges read as straight.
    fn line(&self, window: &mut Window, points: &[Spot], width: f32, color: Hsla) {
        let radius = width / 2.;
        for pair in points.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            let length = (b[0] - a[0]).hypot(b[1] - a[1]);
            // Circles a fifth of their width apart bulge by under a hundredth of it.
            let steps = (length / (width * 0.2)).ceil().max(1.);
            for i in 0..=steps as u16 {
                let q = f32::from(i) / steps;
                let c = [a[0] + (b[0] - a[0]) * q, a[1] + (b[1] - a[1]) * q];
                self.pill(window, c, radius, radius, color);
            }
        }
    }

    /// `paint` kept inside the rectangle from `from` to `to`.
    fn masked(&self, window: &mut Window, from: Spot, to: Spot, paint: impl FnOnce(&mut Window)) {
        let bounds = Bounds::from_corners(self.at(from), self.at(to));
        window.with_content_mask(Some(ContentMask { bounds }), paint);
    }
}

/// The body's four fabrics, each with its print, as the quilt's patterns draw them.
fn body(pen: &Pen, window: &mut Window) {
    let [x0, y0, x1, y1] = BODY;
    let [sx, sy] = SEAM;
    // Each fabric is the whole body cut to its quarter, so the outer corners round as one.
    let whole = [x0, y0, x1 - x0, y1 - y0];
    let patches = [
        ([x0, y0], [sx, sy], 0xff_7eb3),
        ([sx, y0], [x1, sy], 0x5b_c0eb),
        ([x0, sy], [sx, y1], 0x9b_5de5),
        ([sx, sy], [x1, y1], 0x3b_ceac),
    ];
    for (from, to, color) in patches {
        pen.masked(window, from, to, |window| {
            pen.rect(window, whole, BODY_ROUND, hex(color));
        });
    }
    // Pink: dots every 10.
    pen.masked(window, [x0, y0], [sx, sy], |window| {
        for i in 3..8 {
            for j in 3..9 {
                let c = [10. * i as f32 + 5., 10. * j as f32 + 5.];
                if in_body(c, 2.) {
                    pen.pill(window, c, 2., 2., hex(0xff_c4dc));
                }
            }
        }
    });
    // Sky: gingham, bands 6 wide every 12, the level ones fainter.
    pen.masked(window, [sx, y0], [x1, sy], |window| {
        let band = hex(0x8f_d6f3);
        for i in 6..11 {
            let x = 12. * i as f32 + 3.;
            if let Some((a, b)) = clip_to_body([x, y0], [x, sy], 3.) {
                pen.rect(window, [x - 3., a[1], 6., b[1] - a[1]], 0., band);
            }
        }
        for j in 3..8 {
            let y = 12. * j as f32 + 3.;
            if let Some((a, b)) = clip_to_body([sx, y], [x1, y], 3.) {
                pen.rect(
                    window,
                    [a[0], y - 3., b[0] - a[0], 6.],
                    0.,
                    band.opacity(0.7),
                );
            }
        }
    });
    // Violet: stripes 3 wide every 8, turned 45 degrees, as one path.
    pen.masked(window, [x0, sy], [sx, y1], |window| {
        let mut path = PathBuilder::stroke(pen.len(3.));
        for n in 10..20 {
            let c = std::f32::consts::SQRT_2 * (8. * n as f32 + 1.5);
            if let Some((a, b)) = clip_to_body([c - sy, sy], [c - y1, y1], 1.5) {
                path.move_to(pen.at(a));
                path.line_to(pen.at(b));
            }
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, hex(0xb9_8cf0));
        }
    });
    // Mint: a diamond in every 14, as one path.
    pen.masked(window, [sx, sy], [x1, y1], |window| {
        let mut path = PathBuilder::fill();
        for i in 5..10 {
            for j in 6..10 {
                let (x, y) = (14. * i as f32, 14. * j as f32);
                if in_body([x + 7., y + 7.], 5.) {
                    path.add_polygon(
                        &[
                            [x + 7., y + 2.],
                            [x + 9., y + 7.],
                            [x + 7., y + 12.],
                            [x + 5., y + 7.],
                        ]
                        .map(|p| pen.at(p)),
                        true,
                    );
                }
            }
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, hex(0x9f_f0dc));
        }
    });
    // The seams between them: stitches 5 long, 5 apart.
    let seam = gpui::hsla(0., 0., 1., 0.85);
    let mut along = 40.;
    while along < 138. {
        let long = 5_f32.min(138. - along);
        pen.rect(window, [sx - 1.25, along, 2.5, long], 0., seam);
        along += 10.;
    }
    let mut along = 32.;
    while along < 128. {
        let long = 5_f32.min(128. - along);
        pen.rect(window, [along, sy - 1.25, long, 2.5], 0., seam);
        along += 10.;
    }
}

/// Patch, `side` pixels square, doing `act` since the instant with it, and sad when `sad`.
pub(super) fn patch(act: Option<(Act, Instant)>, sad: bool, side: f32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window: &mut Window, cx| {
            let still = cx.reduce_motion() || !window.is_window_active();
            // Which act it is in, and how many seconds in. Only an act draws it again.
            let acting = act
                .map(|(act, since)| (act, since.elapsed().as_secs_f32()))
                .filter(|&(act, e)| e < act.length() && !still);
            if acting.is_some() {
                redraw_after(FRAME, window, cx);
            }

            let hop = match acting {
                Some((Act::Hop | Act::Wave, e)) if e < HOP => {
                    keyed(e / HOP, &[(0., 0.), (0.4, -12.), (1., 0.)])
                }
                _ => 0.,
            };
            // One wave: the arm eases up, sways to and fro, and eases back down, so it never
            // snaps to rest mid-wave.
            let wave = match acting {
                Some((Act::Wave, e)) => {
                    let rise = keyed(e / 0.3, &[(0., 0.), (1., 1.)]);
                    let fall = keyed((Act::Wave.length() - e) / 0.35, &[(0., 0.), (1., 1.)]);
                    let sway = (e * std::f32::consts::TAU / 0.55).sin();
                    rise * fall * (-30. + 14. * sway)
                }
                _ => 0.,
            };
            let knit = match acting {
                Some((Act::Sew, e)) => 16. * (1. - (1. - (e % 0.4) / 0.2).abs()),
                _ => 0.,
            };

            let scale = f32::from(bounds.size.height) / HEIGHT;
            let pen = Pen {
                left: f32::from(bounds.origin.x)
                    + (f32::from(bounds.size.width) - WIDTH * scale) / 2.,
                top: f32::from(bounds.origin.y) + hop * scale,
                scale,
            };

            // The feet: round shoes peeking out under the body, each on a darker sole.
            for x in [57., 103.] {
                pen.pill(window, [x, 146.5], 13., 8., hex(0xc9_9a3a));
                pen.pill(window, [x, 145.], 13., 8., hex(0xf2_c55c));
                pen.pill(window, [x - 3., 142.5], 4.5, 2.2, hex(0xfa_e3a6));
            }

            // The arms: the left works while it sews, the right waves.
            let left = |p: Spot| turn(p, [34., 100.], 20. + knit);
            let arm = [left([21.5, 99.5]), left([32.5, 99.5])];
            pen.line(window, &arm, 15., hex(0x3b_ceac));
            let right = |p: Spot| turn(p, [126., 100.], -20. + wave);
            let arm = [right([127.5, 99.5]), right([138.5, 99.5])];
            pen.line(window, &arm, 15., hex(0x5b_c0eb));

            body(&pen, window);

            // The face, as the agents' avatars have it: a cream patch, tall glossy eyes with a
            // glint each, blushing cheeks, and a small mouth.
            let cream = hex(0xff_f6e8);
            let ink = hex(0x1b_1f33);
            let shine = gpui::hsla(0., 0., 1., 1.);
            let cheek = gpui::hsla(0.95, 0.9, 0.72, 0.55);
            pen.rect(window, [44., 60., 72., 50.], 22., cream);
            for side in [-1., 1.] {
                let eye = [80. + 15. * side, 82.];
                pen.pill(window, eye, 7., 9., ink);
                pen.pill(window, [eye[0] - 2.4, eye[1] - 3.6], 2.6, 2.6, shine);
                pen.pill(window, [eye[0] + 2.4, eye[1] + 3.4], 1.1, 1.1, shine);
                pen.pill(window, [80. + 26. * side, 96.], 6., 3.5, cheek);
            }
            // The mouth: a little bow from one corner to the other, up when sad.
            let (ends, bend) = if sad { (101., 97.) } else { (97., 102.) };
            let mouth: Vec<Spot> = (0..=8_u8)
                .map(|i| {
                    let q = f32::from(i) / 8.;
                    let y = ends + (bend - ends) * 2. * q * (1. - q);
                    [75. + 10. * q, y]
                })
                .collect();
            pen.line(window, &mouth, 2.6, ink);
        },
    )
    .size(px(side))
}

impl super::catalog::QuiltsPage {
    /// Starts Patch on `act`, over whatever it was doing.
    pub(super) fn play(&mut self, act: Act, cx: &mut Context<Self>) {
        self.patch = Some((act, Instant::now()));
        cx.notify();
    }
}
