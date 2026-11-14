//! Themes: the color schemes the app is drawn from, and the Orchestrator page's backdrops.
//!
//! A color scheme is one file in the themes folder and an animated background is one file in
//! the scenes folder. Patchwork comes with some of each, and a person or an agent writes the
//! rest.

// Rust guideline compliant 2026-10-09

mod code;
mod library;
mod prompt;
mod types;

pub use code::{CODE_THEMES, CodeTheme, Syntax};
pub use types::{
    Anchors, Applied, BackdropChoice, CellSize, Color, Dust, Entry, Motion, Overrides, Paint,
    Palette, SavedScene, Scene, SceneFile, Scheme, Shown, Sprite, Theme, ThemeFile,
};
