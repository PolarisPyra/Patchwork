//! Themes: the color schemes the app is drawn from, and the Orchestrator page's backdrops.
//!
//! A color scheme is one file in the themes folder, an animated background is one file in the
//! scenes folder, and a code theme is one file in the code themes folder. Patchwork comes with
//! some of each, which it writes into those folders at launch, and a person or an agent writes
//! the rest.

// Rust guideline compliant 2026-10-09

mod code;
mod contrast;
mod library;
mod prompt;
mod share;
mod types;
mod watch;

pub use code::{CodeTheme, CodeThemeFile, Syntax};
pub use library::init;
pub use share::{Imported, ImportedKind};
pub use types::{
    Anchors, Applied, BackdropChoice, CellSize, Color, Dim, Dust, Entry, Fit, Fonts, Motion,
    Overrides, Paint, Palette, SavedScene, Scene, SceneFile, Scheme, Shown, Sprite, Theme,
    ThemeFile, Unreadable,
};
pub use watch::Watcher;
