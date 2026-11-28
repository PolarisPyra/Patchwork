//! Deleting the files of Settings → Themes and putting the ones Patchwork comes with back as
//! they came: the buttons on a card, and what they do.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::prelude::*;

use super::cards::{DELETE, Tool};
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
        let core = Arc::clone(&self.core);
        let removing = id.clone();
        self.run(
            move || {
                match kind {
                    Filed::Scheme => core.remove_theme(&removing),
                    Filed::Scene => core.remove_scene(&removing),
                    Filed::Code => core.remove_code_theme(&removing),
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
        let core = Arc::clone(&self.core);
        self.close_editor(&id);
        self.run(
            move || {
                match kind {
                    Filed::Scheme => core.reset_theme(&id),
                    Filed::Scene => core.reset_scene(&id),
                    Filed::Code => core.reset_code_theme(&id),
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

/// The buttons and menu rows on the card of a file: Delete on one that was added, and Reset on one Patchwork
/// comes with that was changed.
pub(super) fn file_tools(
    key: &str,
    name: &str,
    kind: Filed,
    id: &str,
    (custom, edited): (bool, bool),
) -> Vec<Tool> {
    let mut tools = Vec::new();
    if edited {
        let id = id.to_string();
        tools.push(Tool::new(
            format!("reset-{key}"),
            ("Reset to built-in", format!("Reset {name} to built-in")),
            patchwork_design_icons::IconName::ArrowCounterClockwise,
            move |d, cx| d.reset_themed(kind, id.clone(), cx),
        ));
    }
    if custom {
        let id = id.to_string();
        tools.push(Tool::new(
            format!("delete-{key}"),
            (DELETE, format!("Delete {name}")),
            patchwork_design_icons::IconName::Trash,
            move |d, cx| d.remove_themed(kind, id.clone(), cx),
        ));
    }
    tools
}
