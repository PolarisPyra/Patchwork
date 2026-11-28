//! Themes: the color schemes the app is drawn from, and the Orchestrator page's backdrops.
//!
//! A color scheme is one file in the themes folder, an animated background is one file in the
//! scenes folder, and a code theme is one file in the code themes folder. Patchwork comes with
//! some of each, which it writes into those folders at launch, and a person or an agent writes
//! the rest.

// Rust guideline compliant 2026-10-09

mod applied;
mod background_sync;
mod foreign;
mod library;
mod preview;
mod prompt;
mod schedule;
mod share;

pub use applied::{Follow, ProjectLook};
pub(crate) use library::{free_theme_id, slug};
pub use library::init;
pub(crate) use preview::preview;
pub(crate) use prompt::THEME_FORMAT;
pub use schedule::{clock_minutes, clock_text};
pub use share::{Imported, ImportedKind};
pub use library::Watcher;
