//! Folding projects and folders away in the task tree, and saving which are.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::prelude::*;
use patchwork_client::AppState;

use crate::Sidebar;

impl Sidebar {
    /// Opens the project or folder `id` if it is folded away, else folds it away.
    pub(crate) fn toggle_collapsed(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_owned());
        }
        self.save_collapsed(cx);
        cx.notify();
    }

    /// Folds every project away.
    pub(crate) fn collapse_all(&mut self, cx: &mut Context<Self>) {
        for project in &AppState::get(cx).settings.projects {
            self.collapsed.insert(project.id.clone());
        }
        self.save_collapsed(cx);
        cx.notify();
    }

    /// Saves which projects and folders are folded away, off the UI thread.
    pub(crate) fn save_collapsed(&self, cx: &gpui::App) {
        let ids: Vec<String> = self.collapsed.iter().cloned().collect();
        let core = Arc::clone(&self.core);
        cx.background_spawn(async move {
            if let Err(e) = core.set_collapsed_sections(&ids) {
                log::warn!(error:% = e; "could not save the open sections: {{error}}");
            }
        })
        .detach();
    }
}
