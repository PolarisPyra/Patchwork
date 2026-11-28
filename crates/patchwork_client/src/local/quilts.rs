//! The quilts in the desktop store.

// Rust guideline compliant 2026-10-09

use std::ops::Deref;
use std::sync::Arc;

use patchwork_core::quilts::Quilt;

use super::AppState;

/// Every quilt.
#[derive(Debug, Clone, Default)]
pub struct QuiltsStore(Arc<Vec<Quilt>>);

impl Deref for QuiltsStore {
    type Target = [Quilt];

    fn deref(&self) -> &[Quilt] {
        &self.0
    }
}

impl AppState {
    /// Keeps `quilts`, read off the UI thread, as the list of every quilt.
    pub fn set_quilts(&mut self, quilts: Vec<Quilt>) {
        self.quilts = QuiltsStore(Arc::new(quilts));
    }
}
