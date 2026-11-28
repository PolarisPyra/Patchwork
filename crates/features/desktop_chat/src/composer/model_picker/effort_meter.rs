//! How hard the model thinks: Auto, then each level the model takes, as the model menu's Effort
//! submenu.

// Rust guideline compliant 2026-10-09

use patchwork_design_text::effort_label;

/// The name of an effort, with no level picked shown as "Auto".
#[must_use]
pub fn effort_name(effort: Option<&str>) -> String {
    effort.map_or_else(|| "Auto".into(), effort_label)
}
