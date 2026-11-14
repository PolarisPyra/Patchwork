//! The orchestrator page's backdrop: a butterfly made of text characters, flying over a field of
//! dust. Nothing is kept between frames; every cell is worked out from the time alone.
//!
//! GPUI cannot blur what is behind an element, so the frosted look of the pill is painted here:
//! a character behind the pill is drawn as a soft glow of its color in place of its glyph.

// Rust guideline compliant 2026-10-09

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "cell coordinates are a few hundred at most"
)]

use std::f32::consts::TAU;
use std::sync::OnceLock;
use std::time::Instant;

use gpui::{
    App, Bounds, BoxShadow, Corners, Font, FontFeatures, Hsla, Pixels, TextAlign, TextRun, Window,
    canvas, fill, font, hsla, point, prelude::*, px, size,
};

use crate::ui::theme::FONT_MONO;

const FONT_SIZE: f32 = 13.;
const CELL_H: f32 = 16.;

/// How often the picture changes. A frame between two changes repaints the lines already shaped.
const FRAMES_PER_SECOND: f32 = 30.;

/// How often the dust twinkles: slower than the butterfly, so most lines stay as they were.
const DUST_PER_SECOND: f32 = 6.;

/// One cell in this many holds a speck of dust.
const DUST_RARITY: u32 = 97;

/// Wing beats a second.
const FLAPS: f32 = 1.6;

/// The trail: how often the butterfly sheds a spark, how many are alight, and how long one lasts.
const SPARK_EVERY: f32 = 0.08;
const SPARKS: i32 = 20;
const SPARK_LIFE: f32 = 1.6;

/// The colors the picture is drawn in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ink {
    DustDim,
    Dust,
    DustBright,
    WingDim,
    Wing,
    WingBright,
    Body,
    Spark,
    SparkFaint,
}

impl Ink {
    fn color(self) -> Hsla {
        match self {
            Self::DustDim => hsla(0.62, 0.30, 0.60, 0.16),
            Self::Dust => hsla(0.62, 0.30, 0.66, 0.34),
            Self::DustBright => hsla(0.58, 0.40, 0.80, 0.70),
            Self::WingDim => hsla(0.65, 0.45, 0.52, 0.75),
            Self::Wing => hsla(0.64, 0.70, 0.70, 1.),
            Self::WingBright => hsla(0.62, 0.80, 0.82, 1.),
            Self::Body => hsla(0.12, 0.30, 0.93, 1.),
            Self::Spark => hsla(0.52, 0.90, 0.78, 0.65),
            Self::SparkFaint => hsla(0.56, 0.80, 0.70, 0.30),
        }
    }
}

/// The page's color under the picture.
fn backdrop() -> Hsla {
    hsla(0.64, 0.30, 0.035, 1.)
}

/// The picture as cells of one character each, empty where nothing is drawn.
struct Grid {
    cols: i32,
    rows: i32,
    cells: Vec<Option<(u8, Ink)>>,
}

impl Grid {
    fn new(cols: i32, rows: i32) -> Self {
        Self {
            cols,
            rows,
            cells: vec![None; (cols * rows) as usize],
        }
    }

    /// Draws a character, over whatever the cell held. A cell off the page is skipped.
    fn put(&mut self, col: i32, row: i32, glyph: u8, ink: Ink) {
        if (0..self.cols).contains(&col) && (0..self.rows).contains(&row) {
            self.cells[(row * self.cols + col) as usize] = Some((glyph, ink));
        }
    }

    fn row(&self, row: i32) -> &[Option<(u8, Ink)>] {
        let start = (row * self.cols) as usize;
        &self.cells[start..start + self.cols as usize]
    }
}

/// The page the butterfly flies over, in cells.
struct Sky {
    cols: f32,
    rows: f32,
    /// How many cells wide a cell is tall, so the butterfly keeps its shape.
    aspect: f32,
}

impl Sky {
    /// Where the butterfly is at `time`: a slow loop around the page, bobbing with each wing beat.
    fn butterfly_at(&self, time: f32) -> (f32, f32) {
        let col = self.cols * (0.5 + 0.36 * (0.21 * time).sin() + 0.06 * (0.83 * time + 1.3).sin());
        let row = self.rows * (0.45 + 0.28 * (0.33 * time + 0.7).sin() + 0.05 * (1.1 * time).sin());
        (col, row + 0.5 * (TAU * FLAPS * time).sin())
    }
}

/// A number that looks random and is the same every time for the same two inputs.
fn scatter(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}

/// Dust: specks that stay where they are and twinkle, each at its own pace.
fn dust(grid: &mut Grid, time: f32) {
    let time = (time * DUST_PER_SECOND).floor() / DUST_PER_SECOND;
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let h = scatter(col as u32, row as u32);
            if !h.is_multiple_of(DUST_RARITY) {
                continue;
            }
            let pace = 0.5 + ((h >> 8) & 7) as f32 * 0.15;
            let phase = ((h >> 12) & 63) as f32 * 0.1;
            let (glyph, ink) = match (time * pace + phase).sin() {
                glow if glow > 0.7 => (b'+', Ink::DustBright),
                glow if glow > 0. => (b'.', Ink::Dust),
                _ => (b'.', Ink::DustDim),
            };
            grid.put(col, row, glyph, ink);
        }
    }
}

/// The sparks the butterfly sheds: each stays near where it was shed, sinks, and fades.
fn sparks(grid: &mut Grid, sky: &Sky, time: f32) {
    let newest = (time / SPARK_EVERY).floor() as i32;
    for spark in (newest - SPARKS..=newest).filter(|n| *n >= 0) {
        let shed = spark as f32 * SPARK_EVERY;
        let age = time - shed;
        if age > SPARK_LIFE {
            continue;
        }
        let h = scatter(spark as u32, 7);
        let (col, row) = sky.butterfly_at(shed);
        let col = col + ((h & 15) as f32 - 7.5) * 0.5;
        let row = row + (((h >> 4) & 7) as f32 - 3.5) * 0.4 + age * 1.4;
        let (glyph, ink) = match age {
            age if age < 0.35 => (b'*', Ink::Spark),
            age if age < 0.9 => (b'+', Ink::Spark),
            _ => (b'.', Ink::SparkFaint),
        };
        grid.put(col.round() as i32, row.round() as i32, glyph, ink);
    }
}

/// The characters a wing is shaded with, from its darkest to its brightest.
const WING_RAMP: &[u8] = b".:-=+*#%";

/// How near its edge a point of a wing is: 0 in the middle of a lobe, 1 on its edge, more outside.
///
/// `out` is how far the point is from the body and `along` how far toward the head, in body
/// lengths.
fn wing_edge(out: f32, along: f32) -> f32 {
    // The forewing and the hindwing, each an ellipse: its middle, the angle of its long axis,
    // and its two half lengths.
    const LOBES: [(f32, f32, f32, f32, f32); 2] = [
        (1.55, 0.95, 0.6, 1.75, 0.95),
        (1.1, -0.75, -0.75, 1.3, 0.95),
    ];
    LOBES
        .iter()
        .map(|&(mid_out, mid_along, angle, long, short)| {
            let (sin, cos) = angle.sin_cos();
            let (out, along) = (out - mid_out, along - mid_along);
            ((out * cos + along * sin) / long).hypot((along * cos - out * sin) / short)
        })
        .fold(f32::INFINITY, f32::min)
}

/// The butterfly: a body with two flat wings hinged on it, seen from behind and a little above,
/// and shaded into characters. It faces the way it is flying and turns around at each end of its
/// loop.
fn butterfly(grid: &mut Grid, sky: &Sky, time: f32) {
    let (center_col, center_row) = sky.butterfly_at(time);
    // A body length, in rows.
    let unit = (sky.rows / 40.).clamp(1.3, 1.9);
    // How far the wings are raised: from just below level to nearly touching above the body.
    let (lift_sin, lift_cos) = (0.55 + 0.8 * (TAU * FLAPS * time).cos()).sin_cos();
    // Which way it faces: to the side it is flying toward, swinging round as it slows.
    let (yaw_sin, yaw_cos) = (0.9 * (3. * (0.21 * time).cos()).tanh()).sin_cos();
    // Its nose is up, and it is looked at from above.
    let (pitch_sin, pitch_cos) = 0.5_f32.sin_cos();
    let (tilt_sin, tilt_cos) = 0.45_f32.sin_cos();
    // Where a point of the butterfly is on the page: to the right, up, and away from the eye.
    let turn = |side: f32, ahead: f32, above: f32| {
        let (ahead, above) = (
            ahead * pitch_cos - above * pitch_sin,
            ahead * pitch_sin + above * pitch_cos,
        );
        let (right, away) = (
            side * yaw_cos + ahead * yaw_sin,
            ahead * yaw_cos - side * yaw_sin,
        );
        (
            right,
            above * tilt_cos + away * tilt_sin,
            away * tilt_cos - above * tilt_sin,
        )
    };
    // How far away the nearest thing drawn in each cell is, so a wing in front hides one behind.
    let mut nearest = vec![f32::INFINITY; grid.cells.len()];
    let mut plot = |(right, up, away): (f32, f32, f32), glyph: u8, ink: Ink| {
        let col = (center_col + right * unit * sky.aspect).round() as i32;
        let row = (center_row - up * unit).round() as i32;
        if (0..grid.cols).contains(&col) && (0..grid.rows).contains(&row) {
            let at = (row * grid.cols + col) as usize;
            if away < nearest[at] {
                nearest[at] = away;
                grid.cells[at] = Some((glyph, ink));
            }
        }
    };

    for side in [-1., 1.] {
        // The way the top of this wing faces. Light comes from the upper left, in front.
        let normal = turn(-side * lift_sin, 0., lift_cos);
        let lit = (-0.4 * normal.0 + 0.7 * normal.1 - 0.6 * normal.2).abs();
        // A wing seen from underneath is in its own shadow.
        let shade = if normal.2 < 0. { 1. } else { 0.5 };
        for out in (0..=60).map(|step| step as f32 * 0.06) {
            for along in (0..=90).map(|step| -2.4 + step as f32 * 0.06) {
                let edge = wing_edge(out, along);
                if edge > 1. {
                    continue;
                }
                let bands = (5. * out + 2. * (3. * along).sin()).sin();
                let light = (0.3 + 0.5 * lit + 0.2 * bands) * (1. - 0.45 * edge * edge) * shade;
                let glyph = WING_RAMP[(light.clamp(0., 0.999) * WING_RAMP.len() as f32) as usize];
                let ink = match light {
                    light if light > 0.7 => Ink::WingBright,
                    light if light > 0.35 => Ink::Wing,
                    _ => Ink::WingDim,
                };
                plot(
                    turn(side * out * lift_cos, along, out * lift_sin),
                    glyph,
                    ink,
                );
            }
        }
    }
    // The body lies just above the wings' hinge, so it shows through where they meet.
    for along in (0..=40).map(|step| -1.3 + step as f32 * 0.0525) {
        let glyph = if along > 0.1 { b'@' } else { b'%' };
        for side in [-0.1, 0., 0.1] {
            let (right, up, away) = turn(side, along, 0.05);
            plot((right, up, away - 0.3), glyph, Ink::Body);
        }
    }
    // The antennae curve up and out from the head, each ending in a knob.
    for side in [-1., 1.] {
        for step in 0..=12 {
            let far = step as f32 / 12.;
            let glyph = if step == 12 { b'@' } else { b'+' };
            let at = turn(side * 0.5 * far, 0.8 + 0.8 * far, 0.45 * far * far);
            plot(at, glyph, Ink::Body);
        }
    }
}

/// Paints the picture at `time` over `bounds`, with a glow for each character behind `frost`.
fn paint(
    bounds: Bounds<Pixels>,
    frost: Bounds<Pixels>,
    time: f32,
    window: &mut Window,
    cx: &mut App,
) {
    window.paint_quad(fill(bounds, backdrop()));
    // The font's ligatures would join "::" and "**" into shapes of their own.
    let mono = Font {
        features: FontFeatures::disable_ligatures(),
        ..font(FONT_MONO)
    };
    let run = |ink: Ink| TextRun {
        len: 1,
        font: mono.clone(),
        color: ink.color(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let measure =
        window
            .text_system()
            .shape_line("0".into(), px(FONT_SIZE), &[run(Ink::Dust)], None);
    let cell_w = f32::from(measure.width);
    if cell_w < 1. {
        return;
    }
    let sky = Sky {
        cols: (f32::from(bounds.size.width) / cell_w).ceil(),
        rows: (f32::from(bounds.size.height) / CELL_H).ceil(),
        aspect: CELL_H / cell_w,
    };
    let mut grid = Grid::new(sky.cols as i32, sky.rows as i32);
    dust(&mut grid, time);
    sparks(&mut grid, &sky, time);
    butterfly(&mut grid, &sky, time);

    let mut glows = Vec::new();
    for row in 0..grid.rows {
        let top = bounds.top() + px(row as f32 * CELL_H);
        let mut text = String::new();
        let mut runs: Vec<TextRun> = Vec::new();
        let mut first = 0;
        let mut last_ink = None;
        for (col, cell) in grid.row(row).iter().enumerate() {
            let place = Bounds::new(
                point(bounds.left() + px(col as f32 * cell_w), top),
                size(px(cell_w), px(CELL_H)),
            );
            let sharp = cell.filter(|(_, ink)| {
                let depth = depth_in_pill(frost, place.center());
                if depth > 0. {
                    glows.push((place.center(), depth, *ink));
                }
                depth <= 0.
            });
            match (sharp, runs.last_mut()) {
                (Some((glyph, ink)), Some(last)) if last_ink == Some(ink) => {
                    text.push(char::from(glyph));
                    last.len += 1;
                }
                (Some((glyph, ink)), _) => {
                    if runs.is_empty() {
                        first = col;
                    }
                    text.push(char::from(glyph));
                    runs.push(run(ink));
                    last_ink = Some(ink);
                }
                // A gap inside the line is a space in the color before it.
                (None, Some(last)) => {
                    text.push(' ');
                    last.len += 1;
                }
                (None, None) => {}
            }
        }
        if runs.is_empty() {
            continue;
        }
        let line = window
            .text_system()
            .shape_line(text.into(), px(FONT_SIZE), &runs, None);
        let origin = point(bounds.left() + px(first as f32 * cell_w), top);
        if let Err(e) = line.paint(origin, px(CELL_H), TextAlign::Left, None, window, cx) {
            log::debug!(error:% = e; "could not paint a line of the background: {{error}}");
        }
    }
    // A glow is as wide as its depth in the pill allows, so it fades out before the pill's edge
    // and nothing has to cut it off there.
    for (center, depth, ink) in glows {
        let half = (depth * 0.3).min(CELL_H / 2.);
        let glow = BoxShadow {
            color: ink.color().opacity(0.5 * (depth / 16.).min(1.)),
            offset: point(px(0.), px(0.)),
            blur_radius: px((depth * 0.45).min(12.)),
            spread_radius: px(0.),
            inset: false,
        };
        let place = Bounds::centered_at(center, size(px(half * 2.), px(half * 2.)));
        window.paint_drop_shadows(place, Corners::all(px(half)), &[glow]);
    }
}

/// How far inside the pill filling `frost` a point is, in pixels. Zero or less is outside it.
fn depth_in_pill(frost: Bounds<Pixels>, at: gpui::Point<Pixels>) -> f32 {
    let radius = f32::from(frost.size.height) / 2.;
    let center = frost.center();
    let reach = (f32::from(frost.size.width) / 2. - radius).max(0.);
    let across = (f32::from(at.x - center.x).abs() - reach).max(0.);
    radius - across.hypot(f32::from(at.y - center.y))
}

/// The animated backdrop, filling its parent. `frost` gives the part of it, such as the pill's
/// bounds, where characters are drawn as glows.
pub(super) fn field(frost: fn(Bounds<Pixels>) -> Bounds<Pixels>) -> impl IntoElement {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    canvas(
        |_, _, _| {},
        move |bounds, (), window: &mut Window, cx| {
            window.request_animation_frame();
            let frame = (start.elapsed().as_secs_f32() * FRAMES_PER_SECOND).floor();
            paint(bounds, frost(bounds), frame / FRAMES_PER_SECOND, window, cx);
        },
    )
    .size_full()
}
