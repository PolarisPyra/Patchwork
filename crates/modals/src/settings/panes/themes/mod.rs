//! Settings → Themes: the color scheme the app is drawn in and the colors of code, and having an
//! agent write a color scheme. It keeps what the background panes and Settings → Interface
//! pick from as well.

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

pub(crate) use backdrops::{chat_background_pane, orchestrator_background_pane, quilts_background_pane};
pub(crate) use interface::interface_pane;
pub(crate) use look::project_look;
use page::{Kind, generator};
pub use page::{ThemesState, code_theme_pane, render};
