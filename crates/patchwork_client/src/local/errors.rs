//! The last error an action hit, as the store keeps it.

// Rust guideline compliant 2026-10-09

use gpui::App;

use super::AppState;

impl AppState {
    /// Shows `error` above the status bar, or clears what is there with `None`.
    pub fn set_error(cx: &mut App, error: Option<String>) {
        Self::update(cx, |state| state.error = error);
    }
}
