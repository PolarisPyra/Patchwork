//! The orchestrator page's backdrop, drawn in text characters on two layers: a sky of dust and
//! stars in the size of code, and over it a butterfly in characters small enough to shade it.
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

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Instant;

use gpui::{
    App, Bounds, BoxShadow, Corners, Font, FontFeatures, Hsla, Pixels, Point, ShapedLine,
    TextAlign, TextRun, Window, canvas, fill, font, hsla, point, prelude::*, px, size,
};

use super::butterfly::{self, Flight};
use super::sky;
use crate::ui::theme::font_mono;

/// The sky's characters, and the butterfly's smaller ones: font size and line height.
pub(super) const SKY: (f32, f32) = (13., 16.);
pub(super) const FLY: (f32, f32) = (8., 10.);

/// How often the picture changes. A frame between two changes repaints the lines already shaped.
pub(super) const FRAMES_PER_SECOND: f32 = 30.;

/// The colors the picture is drawn in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Ink {
    DustFar,
    DustDim,
    Dust,
    DustBright,
    Aurora,
    AuroraViolet,
    Star,
    StarBright,
    Spark,
    SparkFaint,
    WingDeep,
    WingDim,
    Wing,
    WingBright,
    WingHot,
    Border,
    Spot,
    Body,
    BodyDim,
    /// A color a theme's scene names: `0xrrggbb`, how opaque it is of 255, and whether it has
    /// a halo.
    Paint {
        rgb: u32,
        alpha: u8,
        glows: bool,
    },
}

impl Ink {
    pub(super) fn color(self) -> Hsla {
        match self {
            Self::Paint { rgb, alpha, .. } => {
                Hsla::from(gpui::rgb(rgb)).opacity(f32::from(alpha) / 255.)
            }
            Self::DustFar => hsla(0.62, 0.25, 0.55, 0.10),
            Self::DustDim => hsla(0.62, 0.30, 0.60, 0.16),
            Self::Dust => hsla(0.62, 0.30, 0.66, 0.34),
            Self::DustBright => hsla(0.58, 0.40, 0.80, 0.70),
            Self::Aurora => hsla(0.58, 0.70, 0.60, 0.11),
            Self::AuroraViolet => hsla(0.74, 0.60, 0.62, 0.11),
            Self::Star => hsla(0.58, 0.50, 0.85, 0.75),
            Self::StarBright => hsla(0.56, 0.60, 0.95, 1.),
            Self::Spark => hsla(0.52, 0.90, 0.78, 0.65),
            Self::SparkFaint => hsla(0.56, 0.80, 0.70, 0.30),
            Self::WingDeep => hsla(0.66, 0.50, 0.36, 0.85),
            Self::WingDim => hsla(0.65, 0.60, 0.50, 0.95),
            Self::Wing => hsla(0.63, 0.80, 0.64, 1.),
            Self::WingBright => hsla(0.60, 0.85, 0.78, 1.),
            Self::WingHot => hsla(0.58, 0.40, 0.97, 1.),
            Self::Border => hsla(0.67, 0.50, 0.26, 0.95),
            Self::Spot => hsla(0.55, 0.50, 0.90, 1.),
            Self::Body => hsla(0.12, 0.30, 0.93, 1.),
            Self::BodyDim => hsla(0.62, 0.25, 0.62, 1.),
        }
    }

    /// A character this bright has a halo under it.
    fn glows(self) -> bool {
        matches!(
            self,
            Self::WingBright | Self::WingHot | Self::Spot | Self::Paint { glows: true, .. }
        )
    }
}

/// The page's color under the picture.
fn backdrop() -> Hsla {
    hsla(0.64, 0.30, 0.035, 1.)
}

/// A layer of the picture as cells of one character each, empty where nothing is drawn.
pub(super) struct Grid {
    pub(super) cols: i32,
    pub(super) rows: i32,
    /// A cell's size in pixels, so things placed in pixels land in the same spot on every layer.
    pub(super) cell_w: f32,
    pub(super) cell_h: f32,
    pub(super) cells: Vec<Option<(char, Ink)>>,
}

impl Grid {
    pub(super) fn new(width: f32, height: f32, cell_w: f32, cell_h: f32) -> Self {
        let cols = (width / cell_w).ceil() as i32;
        let rows = (height / cell_h).ceil() as i32;
        Self {
            cols,
            rows,
            cell_w,
            cell_h,
            cells: vec![None; (cols * rows).max(0) as usize],
        }
    }

    /// Makes its cells `scale` times their size, so a layer drawn for a whole page is painted
    /// as a small picture of it.
    pub(super) fn shrink(&mut self, scale: f32) {
        self.cell_w *= scale;
        self.cell_h *= scale;
    }

    /// The layer's size in pixels.
    pub(super) fn width(&self) -> f32 {
        self.cols as f32 * self.cell_w
    }

    pub(super) fn height(&self) -> f32 {
        self.rows as f32 * self.cell_h
    }

    /// The cell a point in pixels is in.
    pub(super) fn cell(&self, x: f32, y: f32) -> (i32, i32) {
        (
            (x / self.cell_w).floor() as i32,
            (y / self.cell_h).floor() as i32,
        )
    }

    /// The middle of a cell, in pixels.
    pub(super) fn middle(&self, col: i32, row: i32) -> (f32, f32) {
        (
            (col as f32 + 0.5) * self.cell_w,
            (row as f32 + 0.5) * self.cell_h,
        )
    }

    /// Where a cell is in `cells`. None for a cell off the page.
    pub(super) fn index(&self, col: i32, row: i32) -> Option<usize> {
        ((0..self.cols).contains(&col) && (0..self.rows).contains(&row))
            .then(|| (row * self.cols + col) as usize)
    }

    /// Draws a character, over whatever the cell held. A cell off the page is skipped.
    pub(super) fn put(&mut self, col: i32, row: i32, glyph: char, ink: Ink) {
        if let Some(index) = self.index(col, row) {
            self.cells[index] = Some((glyph, ink));
        }
    }

    /// Draws a character at a point in pixels.
    pub(super) fn put_at(&mut self, x: f32, y: f32, glyph: char, ink: Ink) {
        let (col, row) = self.cell(x, y);
        self.put(col, row, glyph, ink);
    }

    fn row(&self, row: i32) -> &[Option<(char, Ink)>] {
        let start = (row * self.cols) as usize;
        &self.cells[start..start + self.cols as usize]
    }
}

/// A soft round light: where it is, its color, how wide it is and how far it spreads.
struct Glow {
    place: Bounds<Pixels>,
    color: Hsla,
    blur: f32,
}

impl Glow {
    fn paint(&self, window: &mut Window) {
        let shadow = BoxShadow {
            color: self.color,
            offset: point(px(0.), px(0.)),
            blur_radius: px(self.blur),
            spread_radius: px(0.),
            inset: false,
        };
        let round = Corners::all(self.place.size.height / 2.);
        window.paint_drop_shadows(self.place, round, &[shadow]);
    }
}

/// Paints a layer over `bounds` in characters of `font_size`, with a glow for each character
/// behind `frost` and, when `halo` is set, a halo under each run of its brightest characters.
pub(super) fn paint_layer(
    grid: &Grid,
    bounds: Bounds<Pixels>,
    frost: Bounds<Pixels>,
    font_size: f32,
    halo: bool,
    window: &mut Window,
    cx: &mut App,
) {
    // The font's ligatures would join "::" and "**" into shapes of their own.
    let mono = Font {
        features: FontFeatures::disable_ligatures(),
        ..font(font_mono())
    };
    let run = |ink: Ink| TextRun {
        len: 0,
        font: mono.clone(),
        color: ink.color(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut glows = Vec::new();
    let mut lines: Vec<(ShapedLine, Point<Pixels>)> = Vec::new();
    for row in 0..grid.rows {
        let top = bounds.top() + px(row as f32 * grid.cell_h);
        let place = |col: usize, cells: usize| {
            Bounds::new(
                point(bounds.left() + px(col as f32 * grid.cell_w), top),
                size(px(cells as f32 * grid.cell_w), px(grid.cell_h)),
            )
        };
        let mut text = String::new();
        let mut runs: Vec<TextRun> = Vec::new();
        let mut first = 0;
        let mut last_ink = None;
        // The run of bright characters being gathered under one halo: where it starts.
        let mut bright: Option<(usize, Ink)> = None;
        for (col, cell) in grid.row(row).iter().enumerate() {
            let sharp = cell.filter(|(_, ink)| {
                let center = place(col, 1).center();
                let depth = depth_in_pill(frost, center);
                if depth > 0. {
                    // A glow is as wide as its depth in the pill allows, so it fades out before
                    // the pill's edge and nothing has to cut it off there.
                    let radius = (depth * 0.3).min(grid.cell_h / 2.);
                    glows.push(Glow {
                        place: Bounds::centered_at(center, size(px(radius * 2.), px(radius * 2.))),
                        color: ink.color().opacity(0.5 * (depth / 16.).min(1.)),
                        blur: (depth * 0.45).min(12.),
                    });
                }
                depth <= 0.
            });
            let lit = sharp.filter(|(_, ink)| halo && ink.glows());
            match (bright, lit) {
                (None, Some((_, ink))) => bright = Some((col, ink)),
                (Some((from, ink)), None) => {
                    glows.push(Glow {
                        place: place(from, col - from),
                        color: ink.color().opacity(0.30),
                        blur: grid.cell_h,
                    });
                    bright = None;
                }
                _ => {}
            }
            let Some((glyph, ink)) = sharp else {
                // A gap inside the line is a space in the color before it.
                if let Some(last) = runs.last_mut() {
                    text.push(' ');
                    last.len += 1;
                }
                continue;
            };
            if runs.is_empty() {
                first = col;
            }
            if last_ink != Some(ink) {
                runs.push(run(ink));
                last_ink = Some(ink);
            }
            text.push(glyph);
            if let Some(last) = runs.last_mut() {
                last.len += glyph.len_utf8();
            }
        }
        if let Some((from, ink)) = bright {
            glows.push(Glow {
                place: place(from, grid.cols as usize - from),
                color: ink.color().opacity(0.30),
                blur: grid.cell_h,
            });
        }
        if !runs.is_empty() {
            let line = window
                .text_system()
                .shape_line(text.into(), px(font_size), &runs, None);
            lines.push((line, place(first, 1).origin));
        }
    }
    // The lights go under the characters of every line, not only their own.
    for glow in glows {
        glow.paint(window);
    }
    for (line, origin) in lines {
        if let Err(e) = line.paint(origin, px(grid.cell_h), TextAlign::Left, None, window, cx) {
            log::debug!(error:% = e; "could not paint a line of the background: {{error}}");
        }
    }
}

/// How far inside the pill filling `frost` a point is, in pixels. Zero or less is outside it.
pub(super) fn depth_in_pill(frost: Bounds<Pixels>, at: Point<Pixels>) -> f32 {
    let radius = f32::from(frost.size.height) / 2.;
    let center = frost.center();
    let reach = (f32::from(frost.size.width) / 2. - radius).max(0.);
    let across = (f32::from(at.x - center.x).abs() - reach).max(0.);
    radius - across.hypot(f32::from(at.y - center.y))
}

/// The width of one character of the page's code font at this size.
pub(super) fn cell_width(font_size: f32, window: &Window) -> f32 {
    let run = TextRun {
        len: 1,
        font: font(font_mono()),
        color: backdrop(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let measured = window
        .text_system()
        .shape_line("0".into(), px(font_size), &[run], None);
    f32::from(measured.width)
}

/// Seconds since the page's clock started.
pub(super) fn now() -> f32 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

/// A number that looks random and is the same every time for the same two inputs.
pub(super) fn scatter(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}

/// How much smaller than life a page `tall` pixels high is drawn to fill `bounds`. None is a
/// page of the size of `bounds`, drawn as it is.
pub(super) fn scale_of(bounds: Bounds<Pixels>, tall: Option<f32>) -> f32 {
    tall.map_or(1., |tall| f32::from(bounds.size.height) / tall)
}

/// Paints the picture at `time` over `bounds`, with a glow for each character behind `frost`.
/// A `still` picture is the one drawn when the system asks for less motion, and `scale` is how
/// much smaller than the page it is drawn.
#[expect(clippy::too_many_arguments, reason = "one call, from the field")]
fn paint(
    bounds: Bounds<Pixels>,
    frost: Bounds<Pixels>,
    flight: &mut Flight,
    time: f32,
    still: bool,
    scale: f32,
    window: &mut Window,
    cx: &mut App,
) {
    window.paint_quad(fill(bounds, backdrop()));
    if scale <= 0. {
        return;
    }
    let width = f32::from(bounds.size.width) / scale;
    let height = f32::from(bounds.size.height) / scale;
    let (sky_w, fly_w) = (cell_width(SKY.0, window), cell_width(FLY.0, window));
    if sky_w < 1. || fly_w < 1. || width < 1. || height < 1. {
        return;
    }
    flight.step(time, (width, height), still);

    let mut stars = Grid::new(width, height, sky_w, SKY.1);
    sky::draw(&mut stars, time, still);
    flight.wake(&mut stars, time);
    stars.shrink(scale);
    paint_layer(&stars, bounds, frost, SKY.0 * scale, false, window, cx);

    let mut wings = Grid::new(width, height, fly_w, FLY.1);
    butterfly::draw(&mut wings, flight);
    wings.shrink(scale);
    paint_layer(&wings, bounds, frost, FLY.0 * scale, true, window, cx);
}

/// The animated backdrop, filling its parent. `frost` gives the part of it, such as the pill's
/// bounds, where characters are drawn as glows. It reads nothing from the page: `still` holds
/// it still when the system asks for less motion, and `flight` keeps the butterfly's place.
/// With `tall`, it is a page that many pixels high, drawn small enough to fit.
pub(super) fn field(
    frost: fn(Bounds<Pixels>) -> Bounds<Pixels>,
    still: bool,
    flight: Rc<RefCell<Flight>>,
    tall: Option<f32>,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window: &mut Window, cx| {
            if !still {
                window.request_animation_frame();
            }
            let time = (now() * FRAMES_PER_SECOND).floor() / FRAMES_PER_SECOND;
            let mut flight = flight.borrow_mut();
            let scale = scale_of(bounds, tall);
            let frost = frost(bounds);
            paint(bounds, frost, &mut flight, time, still, scale, window, cx);
        },
    )
    .size_full()
}

/// The height of the page a small picture of a backdrop shows, in pixels.
pub(super) const PREVIEW_PAGE: f32 = 240.;

/// The part of a small picture that is behind a pill: none of it.
pub(super) fn no_pill(_: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds::default()
}

/// The butterfly of a small picture of its backdrop, kept from one frame to the next.
#[derive(Clone, Default)]
pub struct PreviewFlight(Rc<RefCell<Flight>>);

/// A small picture of the butterfly's backdrop, filling its parent: the page, drawn smaller.
pub fn butterfly_preview(flight: &PreviewFlight, still: bool) -> impl IntoElement + use<> {
    field(no_pill, still, Rc::clone(&flight.0), Some(PREVIEW_PAGE))
}
