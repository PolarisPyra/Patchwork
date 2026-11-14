//! A theme's scene drawn behind the orchestrator page: a sky, dust over it, and sprites of
//! text characters or colored tiles that move as the theme says.

// Rust guideline compliant 2026-10-09

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "cell coordinates are a few hundred at most"
)]

use std::f32::consts::TAU;
use std::sync::Arc;

use gpui::{
    App, Bounds, BoxShadow, Corners, Hsla, Pixels, Rgba, Window, canvas, fill, linear_color_stop,
    linear_gradient, point, prelude::*, px, size,
};
use patchwork_server::themes::{CellSize, Color, Dust, Motion, SavedScene, Scene, Sprite};

use super::background::{
    FLY, FRAMES_PER_SECOND, Grid, Ink, PREVIEW_PAGE, SKY, cell_width, depth_in_pill, no_pill, now,
    paint_layer, scale_of, scatter,
};
use super::sky;

/// The side of a filled sprite's tile, in pixels: large and small.
const TILE: (f32, f32) = (8., 5.);

/// How often the dust twinkles.
const TWINKLES_PER_SECOND: f32 = 6.;

/// The most of the page's cells that hold a speck of dust.
const MAX_DENSITY: f32 = 0.2;

/// The colors a scene falls back on: the page's own, and the one for characters a sprite
/// gives no color.
#[derive(Clone, Copy)]
pub(super) struct Fallback {
    pub(super) surface: Hsla,
    pub(super) faint: Hsla,
}

fn paint_of(color: Color, alpha: f32) -> Ink {
    Ink::Paint {
        rgb: color.0,
        alpha: (alpha.clamp(0., 1.) * 255.) as u8,
        glows: false,
    }
}

/// A color as the `0xrrggbb` a theme file writes.
fn rgb_of(color: Hsla) -> Color {
    let c = Rgba::from(color);
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u32;
    Color(byte(c.r) << 16 | byte(c.g) << 8 | byte(c.b))
}

/// Whether anything in the scene changes with time, so it needs a frame after this one.
fn moves(scene: &Scene) -> bool {
    let dust = scene
        .dust
        .as_ref()
        .is_some_and(|d| d.twinkle || d.drift != [0., 0.]);
    let sprites = scene
        .sprites
        .iter()
        .any(|s| !matches!(s.motion, Motion::Still));
    dust || sprites || scene.shooting_stars.is_some()
}

/// A sprite's size in pixels when a cell of it is `cell` wide and high.
fn extent(sprite: &Sprite, cell: (f32, f32)) -> (f32, f32) {
    let cols = sprite.art.iter().map(|row| row.chars().count()).max();
    (
        cols.unwrap_or(0) as f32 * cell.0,
        sprite.art.len() as f32 * cell.1,
    )
}

/// Where a sprite's top left corner is at `time`, on a page and with a size in pixels.
fn corner(sprite: &Sprite, time: f32, page: (f32, f32), extent: (f32, f32)) -> (f32, f32) {
    let left = sprite.at[0] * page.0 - extent.0 / 2.;
    let top = sprite.at[1] * page.1 - extent.1 / 2.;
    // Coming back in at the other side once the whole sprite has left.
    let around = |from: f32, moved: f32, page: f32, extent: f32| {
        (from + moved + extent).rem_euclid(page + extent) - extent
    };
    match sprite.motion {
        Motion::Still => (left, top),
        Motion::Bob { height, period } => {
            let turn = time / period.max(0.1);
            (left, top + height * (turn * TAU).sin())
        }
        Motion::Drift { speed } => (
            around(left, speed[0] * time, page.0, extent.0),
            around(top, speed[1] * time, page.1, extent.1),
        ),
        Motion::Wander { speed } => {
            let (across, down) = (page.0 * 0.10, page.1 * 0.08);
            let turn = speed * time / across.max(1.);
            (
                left + across * turn.sin(),
                top + down * (turn * 2.).sin() / 2.,
            )
        }
    }
}

/// The color of a sprite's character.
fn color_of(sprite: &Sprite, glyph: char, fallback: Fallback) -> Color {
    let named = sprite.colors.get(&glyph).copied();
    named.unwrap_or_else(|| rgb_of(fallback.faint))
}

/// A soft light under a sprite that glows, in the color of its first character. `scale` is how
/// much smaller than the page it is drawn.
fn glow(
    sprite: &Sprite,
    place: Bounds<Pixels>,
    fallback: Fallback,
    scale: f32,
    window: &mut Window,
) {
    let first = sprite.art.iter().flat_map(|row| row.chars());
    let Some(glyph) = first.into_iter().find(|c| *c != ' ') else {
        return;
    };
    let color = Hsla::from(gpui::rgb(color_of(sprite, glyph, fallback).0));
    let light = BoxShadow {
        color: color.opacity(0.22),
        offset: point(px(0.), px(0.)),
        blur_radius: px(36. * scale),
        spread_radius: px(-6. * scale),
        inset: false,
    };
    window.paint_drop_shadows(place, Corners::all(place.size.height / 2.), &[light]);
}

/// Paints a filled sprite as tiles. A tile behind `frost` is only a trace of its color.
fn tiles(
    sprite: &Sprite,
    origin: gpui::Point<Pixels>,
    side: f32,
    frost: Bounds<Pixels>,
    fallback: Fallback,
    window: &mut Window,
) {
    for (row, line) in sprite.art.iter().enumerate() {
        for (col, glyph) in line.chars().enumerate() {
            if glyph == ' ' {
                continue;
            }
            let at = point(
                origin.x + px(col as f32 * side),
                origin.y + px(row as f32 * side),
            );
            let tile = Bounds::new(at, size(px(side), px(side)));
            let color = Hsla::from(gpui::rgb(color_of(sprite, glyph, fallback).0));
            let behind = depth_in_pill(frost, tile.center()) > 0.;
            window.paint_quad(fill(tile, color.opacity(if behind { 0.2 } else { 1. })));
        }
    }
}

/// Writes a sprite's characters into the layer, its corner at this point in pixels.
fn write(sprite: &Sprite, grid: &mut Grid, corner: (f32, f32), fallback: Fallback) {
    let (col, row) = grid.cell(corner.0 + grid.cell_w / 2., corner.1 + grid.cell_h / 2.);
    for (down, line) in sprite.art.iter().enumerate() {
        for (across, glyph) in line.chars().enumerate() {
            if glyph != ' ' {
                let ink = paint_of(color_of(sprite, glyph, fallback), 1.);
                grid.put(col + across as i32, row + down as i32, glyph, ink);
            }
        }
    }
}

/// Scatters the dust over the layer as it is at `time`.
fn dust(dust: &Dust, grid: &mut Grid, time: f32) {
    let glyphs: Vec<char> = dust.glyphs.chars().filter(|c| *c != ' ').collect();
    if glyphs.is_empty() || grid.cols < 1 || grid.rows < 1 {
        return;
    }
    let share = (dust.density.clamp(0., MAX_DENSITY) * 10_000.) as u32;
    // The specks move a whole cell at a time, and one leaving a side comes in at the other.
    let moved_cols = (dust.drift[0] * time / grid.cell_w).floor() as i32;
    let moved_rows = (dust.drift[1] * time / grid.cell_h).floor() as i32;
    let beat = (time * TWINKLES_PER_SECOND).floor() / TWINKLES_PER_SECOND;
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let from_col = (col - moved_cols).rem_euclid(grid.cols) as u32;
            let from_row = (row - moved_rows).rem_euclid(grid.rows) as u32;
            let h = scatter(from_col, from_row + 0x5EED);
            if h % 10_000 >= share {
                continue;
            }
            let alpha = if dust.twinkle {
                let pace = 0.5 + ((h >> 8) & 7) as f32 * 0.15;
                let phase = ((h >> 12) & 63) as f32 * 0.1;
                0.6 + 0.4 * (beat * pace + phase).sin()
            } else {
                0.7
            };
            let glyph = glyphs[(h >> 20) as usize % glyphs.len()];
            grid.put(col, row, glyph, paint_of(dust.color, alpha));
        }
    }
}

/// Paints the sky: the theme's color, or two fading from the top to the bottom.
fn sky(scene: &Scene, bounds: Bounds<Pixels>, fallback: Fallback, window: &mut Window) {
    let color = |c: &Color| Hsla::from(gpui::rgb(c.0));
    match scene.sky.as_slice() {
        [] => window.paint_quad(fill(bounds, fallback.surface)),
        [only] => window.paint_quad(fill(bounds, color(only))),
        [top, bottom, ..] => window.paint_quad(fill(
            bounds,
            linear_gradient(
                180.,
                linear_color_stop(color(top), 0.),
                linear_color_stop(color(bottom), 1.),
            ),
        )),
    }
}

/// Paints the scene at `time` over `bounds`, with what is behind `frost` drawn as glows.
/// `scale` is how much smaller than the page it is drawn.
#[expect(clippy::too_many_arguments, reason = "one call, from the field")]
fn paint(
    scene: &Scene,
    bounds: Bounds<Pixels>,
    frost: Bounds<Pixels>,
    fallback: Fallback,
    time: f32,
    scale: f32,
    window: &mut Window,
    cx: &mut App,
) {
    sky(scene, bounds, fallback, window);
    if scale <= 0. {
        return;
    }
    let page = (
        f32::from(bounds.size.width) / scale,
        f32::from(bounds.size.height) / scale,
    );
    let (large_w, small_w) = (cell_width(SKY.0, window), cell_width(FLY.0, window));
    if large_w < 1. || small_w < 1. || page.0 < 1. || page.1 < 1. {
        return;
    }
    let mut large = Grid::new(page.0, page.1, large_w, SKY.1);
    let mut small = Grid::new(page.0, page.1, small_w, FLY.1);
    if let Some(specks) = &scene.dust {
        dust(specks, &mut large, time);
    }
    if let Some(color) = scene.shooting_stars {
        let inks = [1., 0.75, 0.4].map(|alpha| paint_of(color, alpha));
        sky::shooting_star(&mut large, time, inks);
    }
    for sprite in &scene.sprites {
        let is_small = sprite.size == CellSize::Small;
        let cell = match (sprite.filled, is_small) {
            (true, false) => (TILE.0, TILE.0),
            (true, true) => (TILE.1, TILE.1),
            (false, false) => (large_w, SKY.1),
            (false, true) => (small_w, FLY.1),
        };
        let extent = extent(sprite, cell);
        let corner = corner(sprite, time, page, extent);
        let origin = point(
            bounds.left() + px((corner.0 * scale).round()),
            bounds.top() + px((corner.1 * scale).round()),
        );
        if sprite.glow {
            let place = Bounds::new(origin, size(px(extent.0 * scale), px(extent.1 * scale)));
            glow(sprite, place, fallback, scale, window);
        }
        if sprite.filled {
            tiles(sprite, origin, cell.0 * scale, frost, fallback, window);
        } else {
            let grid = if is_small { &mut small } else { &mut large };
            write(sprite, grid, corner, fallback);
        }
    }
    large.shrink(scale);
    small.shrink(scale);
    paint_layer(&large, bounds, frost, SKY.0 * scale, false, window, cx);
    paint_layer(&small, bounds, frost, FLY.0 * scale, false, window, cx);
}

/// A scene filling its parent, read from `holder` each frame; nothing is drawn while it has
/// none. `frost` gives the part of it, such as the pill's bounds, where characters are drawn as
/// glows, and `still` holds it still when the system asks for less motion. With `tall`, it is
/// a page that many pixels high, drawn small enough to fit.
pub(super) fn field<T: 'static>(
    holder: T,
    scene: fn(&T) -> Option<&Scene>,
    fallback: Fallback,
    frost: fn(Bounds<Pixels>) -> Bounds<Pixels>,
    still: bool,
    tall: Option<f32>,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window: &mut Window, cx| {
            let Some(scene) = scene(&holder) else {
                return;
            };
            let still = still || !moves(scene);
            if !still {
                window.request_animation_frame();
            }
            let time = if still {
                0.
            } else {
                (now() * FRAMES_PER_SECOND).floor() / FRAMES_PER_SECOND
            };
            let scale = scale_of(bounds, tall);
            let frost = frost(bounds);
            paint(scene, bounds, frost, fallback, time, scale, window, cx);
        },
    )
    .size_full()
}

/// A small picture of an animated background, filling its parent: the page, drawn smaller.
/// `surface` is the sky of a scene that names none, and `faint` the color of the characters
/// it gives none.
pub fn scene_preview(
    saved: Arc<SavedScene>,
    surface: Hsla,
    faint: Hsla,
    still: bool,
) -> impl IntoElement + use<> {
    let fallback = Fallback { surface, faint };
    field(
        saved,
        saved_scene,
        fallback,
        no_pill,
        still,
        Some(PREVIEW_PAGE),
    )
}

/// The scene of a file.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the field asks for a scene that may be missing"
)]
fn saved_scene(saved: &Arc<SavedScene>) -> Option<&Scene> {
    Some(&saved.file.scene)
}
