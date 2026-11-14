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

use std::f32::consts::{PI, TAU};
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
const FLAPS: f32 = 2.4;

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
    WingRoot,
    WingMid,
    WingOuter,
    WingEdge,
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
            Self::WingRoot => hsla(0.72, 0.70, 0.56, 0.90),
            Self::WingMid => hsla(0.62, 0.90, 0.62, 1.),
            Self::WingOuter => hsla(0.53, 0.95, 0.68, 1.),
            Self::WingEdge => hsla(0.50, 0.70, 0.92, 1.),
            Self::Body => hsla(0.09, 0.85, 0.70, 1.),
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

/// How far a wing reaches from the body at `angle` from straight up, in body lengths.
///
/// This is the butterfly curve, r = e^cos(a) - 2cos(4a). Where it goes negative the wing is on
/// the far side of the body, so the reach is the longer of the two.
fn reach(angle: f32) -> f32 {
    let lobe = |a: f32| a.cos().exp() - 2. * (4. * a).cos();
    lobe(angle).max(-lobe(angle + PI)).max(0.)
}

/// What a wing shows at `angle`, `depth` of the way from the body to its edge.
fn wing(angle: f32, depth: f32) -> (u8, Ink) {
    // The eyespot on each upper wing.
    if (angle - 0.8).abs() < 0.14 && (depth - 0.62).abs() < 0.09 {
        return (b'O', Ink::Body);
    }
    match depth {
        d if d > 0.9 => (b'#', Ink::WingEdge),
        d if d > 0.72 => (b'%', Ink::WingOuter),
        d if d > 0.5 => (b'*', Ink::WingMid),
        d if d > 0.28 => (b':', Ink::WingMid),
        _ => (b'.', Ink::WingRoot),
    }
}

/// The butterfly: its wings, closing and opening, then its body and antennae over them. It leans
/// the way it is flying.
fn butterfly(grid: &mut Grid, sky: &Sky, time: f32) {
    let (center_col, center_row) = sky.butterfly_at(time);
    // A body length, in rows.
    let unit = (sky.rows / 20.).clamp(2.2, 3.2);
    // How open the wings are: seen from above, a closing wing gets narrower.
    let open = 0.15 + 0.85 * (0.5 + 0.5 * (TAU * FLAPS * time).cos());
    let (sin, cos) = (0.3 * (0.21 * time).cos()).sin_cos();
    let reach_rows = (4.2 * unit).ceil() as i32;
    let reach_cols = (4.2 * unit * sky.aspect).ceil() as i32;
    let (mid_col, mid_row) = (center_col.round() as i32, center_row.round() as i32);
    for row in mid_row - reach_rows..=mid_row + reach_rows {
        for col in mid_col - reach_cols..=mid_col + reach_cols {
            let across = (col as f32 - center_col) / sky.aspect / unit;
            let up = (center_row - row as f32) / unit;
            let (x, y) = ((across * cos - up * sin) / open, across * sin + up * cos);
            let angle = x.abs().atan2(y);
            let reach = reach(angle);
            let depth = x.hypot(y) / reach;
            if reach > 0. && depth <= 1. {
                let (glyph, ink) = wing(angle, depth);
                grid.put(col, row, glyph, ink);
            }
        }
    }
    // The cell a point of the butterfly's own frame falls in, leaning with it.
    let cell = |x: f32, y: f32| {
        let (across, up) = (x * cos + y * sin, y * cos - x * sin);
        (
            (center_col + across * unit * sky.aspect).round() as i32,
            (center_row - up * unit).round() as i32,
        )
    };
    let steps = (2.2 * unit * 2.).ceil() as i32;
    for step in 0..=steps {
        let y = -1.4 + 2.2 * step as f32 / steps as f32;
        let (col, row) = cell(0., y);
        grid.put(col, row, if step == steps { b'O' } else { b'8' }, Ink::Body);
    }
    for (x, y, glyph) in [
        (-0.22, 1.15, b'\\'),
        (0.22, 1.15, b'/'),
        (-0.45, 1.5, b'`'),
        (0.45, 1.5, b'\''),
    ] {
        let (col, row) = cell(x, y);
        grid.put(col, row, glyph, Ink::Body);
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
            log::debug!(error:% = e; "could not paint a line of the butterfly: {{error}}");
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
