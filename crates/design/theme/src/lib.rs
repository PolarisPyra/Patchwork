//! Patchwork's look: the palette, sizes and fonts, a child drawn in another scheme, and the files
//! built into the binary that the window draws with.

// Rust guideline compliant 2026-11-27

mod assets;
mod component_theme;
mod in_scheme;
mod palette;
mod sizes;

pub use assets::{Assets, asset, fonts};
pub use in_scheme::{InSchemeElement, in_scheme};
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
