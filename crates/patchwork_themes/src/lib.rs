//! Patchwork's appearance as data: color schemes, animated backgrounds, code themes, and what
//! the appearance settings come to.
//!
//! The core reads and writes these files and settings; the views draw from them. Nothing here
//! reads a file or the settings itself.

// Rust guideline compliant 2026-10-09

mod applied;
mod backdrop;
mod entry;
mod scene;
mod theme_file;

#[doc(inline)]
pub use applied::{Applied, Density, Paint, Palette, TextSize};
#[doc(inline)]
pub use backdrop::{Backdrop, BackdropChoice, Shown};
#[doc(inline)]
pub use theme_file::{CodeTheme, CodeThemeFile, Syntax};
#[doc(inline)]
pub use entry::{Entry, SavedScene, Theme, Unreadable};
#[doc(inline)]
pub use scene::{CellSize, Dust, Motion, SceneFile, Scene, Sprite};
#[doc(inline)]
pub use theme_file::{Anchors, Color, Fonts, Overrides, Scheme, ThemeFile};
