//! Taking theme files into their folders, and saving a color scheme as one file to share.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::*;

use crate::settings::SettingsDialog;

impl SettingsDialog {
    /// Adds each file to the folder it belongs in, and lists what can be picked again.
    pub(super) fn import_themes(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let server = Arc::clone(&self.server);
        self.run(
            move || {
                let failed: Vec<String> = paths
                    .iter()
                    .filter_map(|path| server.import_theme(path).err())
                    .map(|e| e.to_string())
                    .collect();
                (!failed.is_empty()).then(|| failed.join(". "))
            },
            |d, error, cx| {
                d.themes.error = error;
                d.load_themes(cx);
            },
            cx,
        );
    }

    /// Asks for theme files and imports them.
    pub(super) fn pick_themes(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                let _ = this.update(cx, |d, cx| d.import_themes(paths, cx));
            }
        })
        .detach();
    }

    /// Asks where to save a color scheme, and writes it there with its background.
    pub(super) fn export_theme(&mut self, id: String, cx: &mut Context<Self>) {
        let home = patchwork_server::system::fs::home_dir().unwrap_or_default();
        let picked = cx.prompt_for_new_path(&home, Some(&format!("{id}.json")));
        let server = Arc::clone(&self.server);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let exported = cx
                .background_spawn(async move { server.export_theme(&id, &path) })
                .await;
            let _ = this.update(cx, |d, cx| {
                d.themes.error = exported.err().map(|e| e.to_string());
                cx.notify();
            });
        })
        .detach();
    }
}
