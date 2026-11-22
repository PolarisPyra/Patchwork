//! Patchwork's design tokens. Every view reads its colors and sizes from here, never hard-coded.

// Rust guideline compliant 2026-10-09

mod palette;
mod sizes;

#[doc(inline)]
pub use palette::{
    InScheme, Theme, applied, apply, border_focus, follow, glass_over, ink_over, input_placeholder,
    theme,
};
#[doc(inline)]
pub use sizes::{
    ACTIVITY_BAR_W, FONT_MONO, FONT_UI, STATUSBAR_H, TOOLBAR_H, font_mono, font_ui, fs, fs_lg,
    fs_md, fs_sm, fs_xl, fs_xs, fs_xxl, radius, radius_lg, radius_sm, roomy,
};
