//! Patchwork's GPUI design tokens, embedded brand assets, icons and view task helpers.

// Rust guideline compliant 2026-10-09

pub mod brand;
pub mod icons;
mod palette;
mod sizes;
mod tasks;

#[doc(inline)]
pub use brand::{Assets, asset, fonts, logo_file, provider_logo, svgs_changed};
#[doc(inline)]
pub use icons::{Icon, icon};
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
#[doc(inline)]
pub use tasks::{after, off_thread, pick_paths};
