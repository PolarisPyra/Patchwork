//! The name field that renames a row in place.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{AnyElement, Entity, Focusable, KeyDownEvent, Window, div, prelude::*, px};
use patchwork_client::AppState;
use patchwork_design_editor::{Changed, Submitted, TextArea};
use patchwork_design_theme::{radius_sm, theme};

use crate::{Row, Sidebar, SidebarEvent};

/// The name field in place of a row's name.
pub(crate) struct Renaming {
    pub(crate) row: Row,
    field: Entity<TextArea>,
    /// What it was called, so an unchanged name saves nothing.
    current: String,
}

impl Sidebar {
    /// Shows the name field in place of the row's name, holding its name.
    pub fn start_rename(&mut self, row: Row, window: &mut Window, cx: &mut Context<Self>) {
        let state = AppState::get(cx);
        let current = match &row {
            Row::Chat(id) => self.chat_by_id(id, cx).map(|t| t.title.clone()),
            Row::Project(id) => state
                .settings
                .projects
                .iter()
                .find(|p| &p.id == id)
                .map(|p| p.name.clone()),
            Row::Folder(id) => state
                .projects
                .iter()
                .find(|f| &f.id == id)
                .map(|f| f.name.clone()),
            Row::Agent(id) => state
                .agents
                .iter()
                .find(|b| &b.id == id)
                .map(|b| b.name.clone()),
            Row::Archived(_) | Row::Many(_) => None,
        };
        let Some(current) = current else {
            return;
        };
        let field = cx.new(|cx| {
            let mut field = TextArea::new("", "Name", cx);
            field.single_line = true;
            field.set_text(&current, cx);
            field
        });
        cx.subscribe_in(&field, window, |this, _, _: &Submitted, window, cx| {
            this.finish_rename(true, window, cx);
        })
        .detach();
        cx.subscribe(&field, |_, _, _: &Changed, cx| cx.notify())
            .detach();
        let focus = field.read(cx).focus_handle(cx);
        cx.on_blur(&focus, window, |this, window, cx| {
            this.finish_rename(true, window, cx);
        })
        .detach();
        window.focus(&focus, cx);
        self.renaming = Some(Renaming {
            row,
            field,
            current,
        });
        cx.notify();
    }

    /// Saves the typed name, unless it is blank or unchanged, or drops it.
    fn finish_rename(&mut self, save: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.take() else {
            return;
        };
        let name = renaming.field.read(cx).content.trim().to_string();
        let changed = save && !name.is_empty() && name != renaming.current;
        if let (true, Row::Agent(id)) = (changed, &renaming.row) {
            // An agent's page shows its name too.
            cx.emit(SidebarEvent::RenameAgent {
                id: id.clone(),
                name,
            });
        } else if changed {
            let row = renaming.row.clone();
            let rename = move |core: &Arc<patchwork_client::local::core::Core>| match &row {
                Row::Chat(id) => core.chats().rename(id, &name),
                Row::Project(id) => core.rename_project(id, &name),
                Row::Folder(id) => core.rename_folder(id, &name),
                Row::Archived(_) | Row::Many(_) | Row::Agent(_) => Ok(()),
            };
            self.change("rename it", rename, cx);
        }
        if renaming.field.read(cx).focus_handle(cx).is_focused(window) {
            cx.emit(SidebarEvent::FocusWindow);
        }
        self.reload(cx);
    }

    /// The name field, sized to the row it sits in, while `row` is being renamed.
    pub(crate) fn rename_field(&self, row: &Row, cx: &mut Context<Self>) -> Option<AnyElement> {
        let renaming = self.renaming.as_ref().filter(|r| &r.row == row)?;
        let t = *theme(cx);
        Some(
            div()
                .id("rename")
                .h(px(28.))
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .px(px(4.))
                .rounded(px(radius_sm()))
                .border_1()
                .border_color(t.accent)
                .bg(t.bg_input)
                .on_click(|_, _, cx| cx.stop_propagation())
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                    if e.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.finish_rename(false, window, cx);
                    }
                }))
                .child(renaming.field.clone())
                .into_any_element(),
        )
    }
}
