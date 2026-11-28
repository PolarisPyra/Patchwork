//! The open menu, and reading the files and skills it lists.

// Rust guideline compliant 2026-10-09

use patchwork_client::AppState;
use super::commands::Item;
use super::commands::Token;
use std::sync::Arc;
use gpui::{Entity, ScrollHandle, Subscription, prelude::*};
use patchwork_design_components::menu::{Dismissed, Menu};
use crate::composer::Composer;
use patchwork_views_chat::sigils::SigilKind;

/// The host's places, in the app's menu: what it was made for, and the menu.
pub(super) struct Mentions {
    /// The word typed after `@` and the ids listed, which make a new menu when they change.
    pub(super) made_for: (String, Vec<String>),
    pub(super) menu: Entity<Menu>,
    _dismissed: Subscription,
}

impl Mentions {
    pub(super) fn new(
        made_for: (String, Vec<String>),
        menu: Entity<Menu>,
        cx: &mut Context<Composer>,
    ) -> Self {
        // A press outside hides it, as Escape does, until the draft changes.
        let dismissed = cx.subscribe(&menu, |this, _, e: &Dismissed, cx| {
            if e.0.is_some() {
                this.sigil.closed = true;
                cx.notify();
            }
        });
        Self {
            made_for,
            menu,
            _dismissed: dismissed,
        }
    }
}

#[derive(Default)]
pub struct SigilMenu {
    /// Escape hid the menu; it shows again once the draft changes.
    pub(super) closed: bool,
    pub(super) highlight: usize,
    /// The kind of word last under the caret, and the project it was typed in: its list is read
    /// again each time a menu opens, and when the project changes under it.
    pub(super) kind: Option<(SigilKind, Option<String>)>,
    pub(super) files: Vec<String>,
    /// The menu `@` opens when the host lists its own places in place of files.
    pub(super) mentions: Option<Mentions>,
    /// The enabled skills' names and descriptions.
    pub(super) skills: Vec<(String, Option<String>)>,
    /// The word and entries of the open menu, as last drawn.
    pub(super) open: Option<(Token, Vec<Item>)>,
    pub(super) scroll: ScrollHandle,
}

impl Composer {
    /// The draft changed: the menu shows again, from the top, for the word now typed.
    pub(in crate::composer) fn draft_changed(&mut self) {
        self.sigil.closed = false;
        self.sigil.highlight = 0;
    }

    pub(super) fn load_files(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.host.project(cx).map(|p| p.id.clone()) else {
            self.sigil.files.clear();
            return;
        };
        let core = Arc::clone(AppState::get(cx).core());
        let listed = project.clone();
        patchwork_design_tasks::off_thread(
            cx,
            move || core.list_files(&project),
            move |this, files, cx| {
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
            },
        );
    }

    pub(super) fn load_skills(&mut self, cx: &mut Context<Self>) {
        let settings = AppState::get(cx).settings.shared();
        let task = cx.background_spawn(async move {
            patchwork_client::local::core::extensions::skills::all(&settings)
        });
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
