//! Keeping quilts: pinning one to the gallery's top, renaming it, deleting it, and the menu
//! that does each from a card.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{Pixels, Point, PromptLevel, Window, prelude::*};

use super::state::QuiltsPage;
use crate::ui::menu::{Menu, MenuItem};
use crate::ui::off_thread;
use crate::ui::text_area::{Submitted, TextArea};

impl QuiltsPage {
    /// Opens the menu of the quilt `id` at `at`: pin or unpin, rename, and delete.
    pub(super) fn open_menu(
        &mut self,
        id: &str,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(quilt) = self.list.iter().find(|a| a.id == id) else {
            return;
        };
        let pinned = quilt.pinned;
        let this = cx.entity().downgrade();
        let item = |label: &str,
                    icon: &'static str,
                    act: fn(&mut Self, &str, &mut Window, &mut Context<Self>)| {
            let (this, id) = (this.clone(), id.to_owned());
            MenuItem::new(label.to_owned(), move |window, cx| {
                let _ = this.update(cx, |this, cx| act(this, &id, window, cx));
            })
            .icon(icon)
        };
        let items = vec![
            if pinned {
                item("Unpin", "push-pin-slash", |this, id, _, cx| {
                    this.pin(id.to_owned(), false, cx);
                })
            } else {
                item("Pin", "push-pin", |this, id, _, cx| {
                    this.pin(id.to_owned(), true, cx);
                })
            },
            item("Rename", "pencil-simple", Self::start_rename),
            item("Delete", "trash", |this, id, window, cx| {
                this.delete(id.to_owned(), window, cx);
            })
            .separated()
            .danger(),
        ];
        let menu = Menu::open_for(
            quilt.title.clone(),
            items,
            window,
            cx,
            |this, window, cx| {
                this.menu = None;
                window.focus(&this.focus, cx);
                cx.notify();
            },
        );
        self.menu = Some((menu, at));
        cx.notify();
    }

    /// Pins the quilt `id` to the gallery's top, or unpins it.
    pub(super) fn pin(&mut self, id: String, pinned: bool, cx: &mut Context<Self>) {
        if pinned {
            self.play(super::patch::Act::Hop, cx);
        }
        let core = Arc::clone(&self.core);
        // The core tells the window, which reads them again.
        off_thread(cx, move || core.pin_quilt(&id, pinned), Self::failed);
    }

    /// Opens the quilt `id` with a field in place of its title, which Enter saves and Escape
    /// leaves.
    pub(super) fn start_rename(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.as_deref() != Some(id) {
            self.select(id.to_owned(), cx);
        }
        let Some(title) = self
            .list
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.title.clone())
        else {
            return;
        };
        let field = cx.new(|cx| {
            let mut field = TextArea::new("Title", "Title", cx);
            field.single_line = true;
            field.set_text(&title, cx);
            field
        });
        cx.subscribe(&field, |this, _, _: &Submitted, cx| this.finish_rename(cx))
            .detach();
        let focus = gpui::Focusable::focus_handle(field.read(cx), cx);
        window.focus(&focus, cx);
        self.renaming = Some(field);
        cx.notify();
    }

    /// Saves the title typed for the opened quilt.
    pub(super) fn finish_rename(&mut self, cx: &mut Context<Self>) {
        let (Some(field), Some(id)) = (self.renaming.take(), self.selected.clone()) else {
            return;
        };
        let title = field.read(cx).content.trim().to_owned();
        cx.notify();
        if title.is_empty() || self.current().is_some_and(|a| a.title == title) {
            return;
        }
        let core = Arc::clone(&self.core);
        off_thread(cx, move || core.rename_quilt(&id, &title), Self::failed);
    }

    /// Leaves the title as it was.
    pub(super) fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.renaming = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Asks, then deletes the quilt `id` with every version, going back to the gallery when it
    /// was the one opened.
    pub(super) fn delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(quilt) = self.list.iter().find(|a| a.id == id) else {
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Delete \u{201c}{}\u{201d}?", quilt.title),
            Some("Every version of it is deleted too."),
            &["Delete", "Cancel"],
            cx,
        );
        let core = Arc::clone(&self.core);
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update(cx, |_, cx| {
                off_thread(
                    cx,
                    move || core.delete_quilt(&id).map(|()| id),
                    |this, deleted, cx| match deleted {
                        // The core tells the window, which reads them again.
                        Ok(id) if this.selected.as_ref() == Some(&id) => this.close(cx),
                        done => this.failed(done.map(drop), cx),
                    },
                );
            });
        })
        .detach();
    }

    /// Shows what went wrong, when something did.
    fn failed(&mut self, done: Result<(), patchwork_core::system::Error>, cx: &mut Context<Self>) {
        if let Err(e) = done {
            log::warn!(error:% = e; "could not change a quilt: {{error}}");
            self.error = Some(e.to_string());
            cx.notify();
        }
    }
}
