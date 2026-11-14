//! The orchestrator page's backdrop: a field of dust made of text characters, twinkling. Nothing
//! is kept between frames; every cell is worked out from the time alone.
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

use std::sync::OnceLock;
use std::time::Instant;

use gpui::{
    App, Bounds, BoxShadow, Corners, Font, FontFeatures, Hsla, Pixels, TextAlign, TextRun, Window,
    canvas, fill, font, hsla, point, prelude::*, px, size,
};

use crate::ui::theme::FONT_MONO;

const FONT_SIZE: f32 = 13.;
const CELL_H: f32 = 16.;

/// How often the dust twinkles, so most lines stay as they were from one frame to the next.
const DUST_PER_SECOND: f32 = 6.;

/// One cell in this many holds a speck of dust.
const DUST_RARITY: u32 = 97;

/// The colors the picture is drawn in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ink {
    DustDim,
    Dust,
    DustBright,
}

impl Ink {
    fn color(self) -> Hsla {
        match self {
            Self::DustDim => hsla(0.62, 0.30, 0.60, 0.16),
            Self::Dust => hsla(0.62, 0.30, 0.66, 0.34),
            Self::DustBright => hsla(0.58, 0.40, 0.80, 0.70),
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
    let cols = (f32::from(bounds.size.width) / cell_w).ceil();
    let rows = (f32::from(bounds.size.height) / CELL_H).ceil();
    let mut grid = Grid::new(cols as i32, rows as i32);
    dust(&mut grid, time);

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
            log::debug!(error:% = e; "could not paint a line of the dust: {{error}}");
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
            paint(
                bounds,
                frost(bounds),
                start.elapsed().as_secs_f32(),
                window,
                cx,
            );
        },
    )
    .size_full()
}
