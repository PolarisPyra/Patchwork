//! The sky behind the butterfly: dust under an aurora, and now and then a shooting star.

// Rust guideline compliant 2026-10-09

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "cell coordinates are a few hundred at most"
)]

use super::background::{Grid, Ink, scatter};

/// How often the dust twinkles: slower than the butterfly, so most lines stay as they were.
const DUST_PER_SECOND: f32 = 6.;

/// The layers of dust, the farthest first: how many cells hold one speck, and whether its
/// specks twinkle. None of them moves.
const DUST: [(u32, bool); 3] = [(61, false), (131, false), (173, true)];

/// How often a shooting star may cross, and how long one takes.
const SHOOT_EVERY: f32 = 9.;
const SHOOT: f32 = 0.8;

/// Draws the sky at `time`. A `still` sky neither twinkles nor has shooting stars.
pub(super) fn draw(grid: &mut Grid, time: f32, still: bool) {
    aurora(grid);
    dust(grid, if still { 0. } else { time });
    if !still {
        shooting_star(grid, time, [Ink::StarBright, Ink::Star, Ink::Dust]);
    }
}

/// The aurora: dim bands in the top third of the page, blue in places and violet in others.
/// It holds still.
fn aurora(grid: &mut Grid) {
    let band = grid.rows / 3;
    for row in 0..band {
        let fade = 1. - row as f32 / band as f32;
        let sway = 1.7 * (row as f32 * 0.35).sin();
        for col in 0..grid.cols {
            let wave = (col as f32 * 0.05 + sway).sin() * fade;
            if wave < 0.5 || !scatter(col as u32, row as u32 + 9000).is_multiple_of(3) {
                continue;
            }
            let violet = (col as f32 * 0.02).sin() > 0.;
            let ink = if violet {
                Ink::AuroraViolet
            } else {
                Ink::Aurora
            };
            grid.put(col, row, if wave > 0.72 { ':' } else { '-' }, ink);
        }
    }
}

/// Dust in three layers that stay where they are, the nearest twinkling.
fn dust(grid: &mut Grid, time: f32) {
    let twinkle = (time * DUST_PER_SECOND).floor() / DUST_PER_SECOND;
    for (layer, &(rarity, twinkles)) in DUST.iter().enumerate() {
        for row in 0..grid.rows {
            for col in 0..grid.cols {
                let h = scatter(col as u32, row as u32 + layer as u32 * 4096);
                if !h.is_multiple_of(rarity) {
                    continue;
                }
                let (glyph, ink) = if twinkles {
                    let pace = 0.5 + ((h >> 8) & 7) as f32 * 0.15;
                    let phase = ((h >> 12) & 63) as f32 * 0.1;
                    match (twinkle * pace + phase).sin() {
                        glow if glow > 0.7 => ('+', Ink::DustBright),
                        glow if glow > 0. => ('.', Ink::Dust),
                        _ => ('.', Ink::DustDim),
                    }
                } else if layer == 0 {
                    ('.', Ink::DustFar)
                } else {
                    ('.', Ink::DustDim)
                };
                grid.put(col, row, glyph, ink);
            }
        }
    }
}

/// A shooting star: now and then a short streak crosses one of the top corners, in these
/// colors from its head to the end of its tail.
pub(super) fn shooting_star(grid: &mut Grid, time: f32, inks: [Ink; 3]) {
    let slot = (time / SHOOT_EVERY).floor();
    let h = scatter(slot as u32, 0xA57);
    let along = (time - slot * SHOOT_EVERY) / SHOOT;
    if !h.is_multiple_of(3) || along > 1.3 {
        return;
    }
    let (width, height) = (grid.width(), grid.height());
    let side = if h & 8 == 0 { 0. } else { 1. };
    let start = 0.16 + ((h >> 4) & 15) as f32 * 0.01;
    for tail in 0..9 {
        let u = along - tail as f32 * 0.035;
        if !(0. ..1.).contains(&u) {
            continue;
        }
        // From the top edge near the corner down to the side edge.
        let across = start * (1. - u);
        let x = width * (side + (1. - 2. * side) * across);
        let y = height * (0.02 + 0.26 * u);
        let (glyph, ink) = match tail {
            0 => ('=', inks[0]),
            1..=3 => ('-', inks[1]),
            _ => ('.', inks[2]),
        };
        grid.put_at(x, y, glyph, ink);
    }
}
