//! What is drawn behind a page: the presets, and the animated backdrops in text characters.
//!
//! [`presets`] are the gradients, dots and grid the app comes with. [`field`] draws the sky and
//! the butterfly over it ([`sky`], [`butterfly`]), and [`scene`] a theme's own scene. The
//! orchestrator page, a chat, and the pickers in Settings and beside a chat all draw them.

// Rust guideline compliant 2026-10-09

mod butterfly;
mod field;
mod presets;
mod scene;
mod sky;

pub use butterfly::Flight;
pub use field::{PreviewFlight, butterfly_backdrop, butterfly_field, butterfly_preview};
pub use presets::{PRESETS, preset_fill};
pub use scene::{Fallback, scene_backdrop, scene_field, scene_preview};
