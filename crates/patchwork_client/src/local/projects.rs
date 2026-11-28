//! The projects' folders in the desktop store.

// Rust guideline compliant 2026-10-09

use std::ops::Deref;
use std::sync::Arc;

use patchwork_core::Core;
use patchwork_core::projects::Folder;

use super::AppState;

/// The projects' folders.
#[derive(Debug, Clone, Default)]
pub struct ProjectsStore(Arc<Vec<Folder>>);

impl ProjectsStore {
    pub(super) fn read(core: &Core) -> Self {
        Self(Arc::new(core.list_folders()))
    }
}

impl Deref for ProjectsStore {
    type Target = [Folder];

    fn deref(&self) -> &[Folder] {
        &self.0
    }
}

impl AppState {
    /// Reads the projects' folders again.
    pub fn reload_folders(&mut self) {
        self.projects = ProjectsStore::read(&self.core);
    }
}
