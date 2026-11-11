//! The open menu, and reading the files and skills it lists.

// Rust guideline compliant 2026-10-09

use super::commands::Item;
use super::commands::Token;
use std::sync::Arc;
use gpui::{ScrollHandle, prelude::*};
use crate::chat::composer::Composer;
use crate::ui::sigils::SigilKind;

#[derive(Default)]
pub struct SigilMenu {
    /// Escape hid the menu; it shows again once the draft changes.
    pub(super) closed: bool,
    pub(super) highlight: usize,
    /// The kind of word last under the caret, and the project it was typed in: its list is read
    /// again each time a menu opens, and when the project changes under it.
    pub(super) kind: Option<(SigilKind, Option<String>)>,
    pub(super) files: Vec<String>,
    /// The enabled skills' names and descriptions.
    pub(super) skills: Vec<(String, Option<String>)>,
    /// The word and entries of the open menu, as last drawn.
    pub(super) open: Option<(Token, Vec<Item>)>,
    pub(super) scroll: ScrollHandle,
}

impl Composer {
    /// The draft changed: the menu shows again, from the top, for the word now typed.
    pub(in crate::chat::composer) fn draft_changed(&mut self) {
        self.sigil.closed = false;
        self.sigil.highlight = 0;
    }

    pub(super) fn load_files(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let w = workspace.read(cx);
        let Some(project) = w.project().map(|p| p.id.clone()) else {
            self.sigil.files.clear();
            return;
        };
        let server = Arc::clone(&w.server);
        let listed = project.clone();
        let task = cx.background_spawn(async move { server.list_files(&project) });
        cx.spawn(async move |this, cx| {
            let files = task.await;
            this.update(cx, |this, cx| {
                // A listing that arrives after the project changed is another project's files.
                let current = this
                    .sigil
                    .kind
                    .as_ref()
                    .and_then(|(_, project)| project.as_ref());
                if current != Some(&listed) {
                    return;
                }
                match files {
                    Ok(files) => this.sigil.files = files,
                    Err(e) => {
                        log::warn!(error:% = e; "could not list the project's files: {{error}}");
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    pub(super) fn load_skills(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let settings = workspace.read(cx).settings.clone();
        let task = cx.background_spawn(async move { patchwork_server::skills::all(&settings) });
        cx.spawn(async move |this, cx| {
            let skills = task.await;
            this.update(cx, |this, cx| {
                this.sigil.skills = skills
                    .into_iter()
                    .filter(|s| s.enabled)
                    .map(|s| (s.name, s.description))
                    .collect();
                cx.notify();
            })
        })
        .detach();
    }
}
