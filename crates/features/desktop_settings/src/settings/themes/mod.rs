//! Settings → Appearance: light or dark, the accent, the palettes, the colors of code, the
//! backgrounds, the interface, and having an agent write a palette, code theme or background.

// Rust guideline compliant 2026-10-09

mod backdrops;
mod cards;
mod editor;
mod files;
mod generator_pick;
mod interface;
mod look;
mod page;
mod reading;
mod share;
mod slider;

use backdrops::{chat_background_pane, orchestrator_background_pane, quilts_background_pane};
use interface::interface_pane;
pub(crate) use look::project_look;
use page::{Kind, generator};
pub use page::{ThemesState, appearance};
