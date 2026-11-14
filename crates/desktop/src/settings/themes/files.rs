//! Deleting the files of Settings → Themes and putting the ones Patchwork comes with back as
//! they came: the buttons on a card, and what they do.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{AnyElement, prelude::*};

use super::cards::tool;
use crate::settings::SettingsDialog;

/// The kinds of file a card stands for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Filed {
    Scheme,
    Scene,
    Code,
}

impl SettingsDialog {
    /// Deletes a file. A setting that picked it goes back to what it is without one.
    fn remove_themed(&mut self, kind: Filed, id: String, cx: &mut Context<Self>) {
        let server = Arc::clone(&self.server);
        let removing = id.clone();
        self.run(
            move || {
                match kind {
                    Filed::Scheme => server.remove_theme(&removing),
                    Filed::Scene => server.remove_scene(&removing),
                    Filed::Code => server.remove_code_theme(&removing),
                }
                .map_err(|e| e.to_string())
            },
            move |d, removed, cx| {
                d.themes.error = removed.err();
                d.close_editor(&id);
                d.update(
                    |s| match kind {
                        Filed::Scheme => s.drop_theme(&id),
                        Filed::Scene => s.drop_scene(&id),
                        Filed::Code => s.drop_code_theme(&id),
                    },
                    cx,
                );
                d.load_themes(cx);
            },
            cx,
        );
    }

    /// Writes a file Patchwork comes with as it came, over the changes made to it.
    fn reset_themed(&mut self, kind: Filed, id: String, cx: &mut Context<Self>) {
        let server = Arc::clone(&self.server);
        self.close_editor(&id);
        self.run(
            move || {
                match kind {
                    Filed::Scheme => server.reset_theme(&id),
                    Filed::Scene => server.reset_scene(&id),
                    Filed::Code => server.reset_code_theme(&id),
                }
                .map_err(|e| e.to_string())
            },
            |d, reset, cx| {
                d.themes.error = reset.err();
                d.load_themes(cx);
            },
            cx,
        );
    }
}

/// The buttons on the card of a file: Delete on one that was added, and Reset on one Patchwork
/// comes with that was changed.
pub(super) fn file_tools(
    key: &str,
    name: &str,
    kind: Filed,
    id: &str,
    (custom, edited): (bool, bool),
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let mut tools = Vec::new();
    if edited {
        let id = id.to_string();
        tools.push(tool(
            format!("reset-{key}"),
            format!("Reset {name} to built-in"),
            "arrow-counter-clockwise",
            move |d, cx| d.reset_themed(kind, id.clone(), cx),
            cx,
        ));
    }
    if custom {
        let id = id.to_string();
        tools.push(tool(
            format!("delete-{key}"),
            format!("Delete {name}"),
            "trash",
            move |d, cx| d.remove_themed(kind, id.clone(), cx),
            cx,
        ));
    }
    tools
}
