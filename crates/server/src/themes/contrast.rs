//! Whether a color scheme's text can be read on its surface.

// Rust guideline compliant 2026-10-09

use super::types::{Color, ThemeFile};

/// The least contrast the main text needs on the surface, and the least the faint text needs.
const TEXT: f32 = 4.5;
const MUTED: f32 = 3.;

/// How bright a color looks, from 0 for black to 1 for white, as WCAG 2 measures it.
fn luminance(color: Color) -> f32 {
    let channel = |shift: u32| {
        let c = f32::from(u8::try_from((color.0 >> shift) & 0xff).unwrap_or(u8::MAX)) / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

/// The contrast of two colors, from 1 for the same color to 21 for black on white.
fn ratio(a: Color, b: Color) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// What is hard to read in a color scheme: each text whose contrast on the surface is too low.
pub(super) fn notes(file: &ThemeFile) -> Vec<String> {
    let colors = file.colors;
    [
        ("Text", colors.text, TEXT),
        ("Faint text", colors.muted, MUTED),
    ]
    .into_iter()
    .filter_map(|(what, color, least)| {
        let contrast = ratio(color, colors.surface);
        (contrast < least).then(|| {
            format!("{what} is hard to read: its contrast is {contrast:.1}, under {least}")
        })
    })
    .collect()
}
