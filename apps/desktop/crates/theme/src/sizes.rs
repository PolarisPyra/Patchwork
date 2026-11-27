//! The fonts, corner radii and text sizes in use, and the fixed sizes of the bars.

// Rust guideline compliant 2026-10-09

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{PoisonError, RwLock};

use gpui::SharedString;

/// The font families and the corner radius in use: the app's own, or the color scheme's.
///
/// They are read where no `App` is at hand, so they are kept beside the `Theme` global, and
/// `apply` sets both.
pub(super) static FONTS: RwLock<[SharedString; 2]> = RwLock::new([
    SharedString::new_static(FONT_UI),
    SharedString::new_static(FONT_MONO),
]);
pub(super) static CORNER: AtomicU32 = AtomicU32::new(RADIUS.to_bits());
pub(super) static DENSITY: AtomicU32 = AtomicU32::new(1_f32.to_bits());
pub(super) static TEXT_SCALE: AtomicU32 = AtomicU32::new(1_f32.to_bits());

/// A row's height or its padding at the density in Settings: `size` is the comfortable one.
pub fn roomy(size: f32) -> f32 {
    (size * f32::from_bits(DENSITY.load(Ordering::Relaxed))).round()
}

pub(super) fn font_family(index: usize) -> SharedString {
    FONTS.read().unwrap_or_else(PoisonError::into_inner)[index].clone()
}

/// The font family of the app's own text.
#[must_use]
pub fn font_ui() -> SharedString {
    font_family(0)
}

/// The font family of code and terminals.
#[must_use]
pub fn font_mono() -> SharedString {
    font_family(1)
}

/// The corner radius of a button or a field.
pub fn radius() -> f32 {
    f32::from_bits(CORNER.load(Ordering::Relaxed))
}

/// The corner radius of something small, such as a chip.
#[must_use]
pub fn radius_sm() -> f32 {
    radius() * RADIUS_SM / RADIUS
}

/// The corner radius of something large, such as a dialog.
#[must_use]
pub fn radius_lg() -> f32 {
    radius() * RADIUS_LG / RADIUS
}

/// The font families the app comes with, and its sizes, from `theme.css`'s `:root`.
pub const FONT_UI: &str = "Inter";
pub const FONT_MONO: &str = "JetBrains Mono";
pub(super) const RADIUS_SM: f32 = 4.;
pub(super) const RADIUS: f32 = 6.;
pub(super) const RADIUS_LG: f32 = 10.;
/// The roundest a color scheme may make a button's corners.
pub(super) const MAX_RADIUS: f32 = 16.;
pub const TOOLBAR_H: f32 = 40.;
pub const STATUSBAR_H: f32 = 27.;
pub const ACTIVITY_BAR_W: f32 = 40.;

/// What the sizes of the app's text are multiplied by: the text size in Settings.
pub fn text_scale() -> f32 {
    f32::from_bits(TEXT_SCALE.load(Ordering::Relaxed))
}

/// A text size at the text size in Settings: `size` is the medium one.
#[must_use]
pub fn fs(size: f32) -> f32 {
    size * text_scale()
}

/// The sizes of the app's text, from `theme.css`'s `:root`, at the text size in Settings.
#[must_use]
pub fn fs_xs() -> f32 {
    fs(11.)
}

#[must_use]
pub fn fs_sm() -> f32 {
    fs(12.)
}

#[must_use]
pub fn fs_md() -> f32 {
    fs(13.)
}

#[must_use]
pub fn fs_lg() -> f32 {
    fs(15.)
}

#[must_use]
pub fn fs_xl() -> f32 {
    fs(20.)
}

#[must_use]
pub fn fs_xxl() -> f32 {
    fs(28.)
}
