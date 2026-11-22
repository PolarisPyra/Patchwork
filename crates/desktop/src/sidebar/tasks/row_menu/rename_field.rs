//! The name field that renames a row in place.

// Rust guideline compliant 2026-10-09

use super::target::Renaming;
use super::target::Target;
use gpui::{AnyElement, Focusable, KeyDownEvent, Window, div, prelude::*, px};
use patchwork_ui::text::text_area::{Changed, Submitted, TextArea};
use patchwork_ui::theme::{radius_sm, theme};
use crate::workspace::Workspace;

impl Workspace {
    /// Shows the name field in place of the row's name, holding its name.
    pub fn start_rename(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let current = match &target {
            Target::Chat(id) => self.chat_by_id(id).map(|t| t.title.clone()),
            Target::Project(id) => self
                .settings
                .projects
                .iter()
                .find(|p| &p.id == id)
                .map(|p| p.name.clone()),
            Target::Folder(id) => self
                .folders
                .iter()
                .find(|f| &f.id == id)
                .map(|f| f.name.clone()),
            Target::Agent(id) => self
                .agents
                .iter()
                .find(|b| &b.id == id)
                .map(|b| b.name.clone()),
            Target::Archived(_) | Target::Many(_) => None,
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
            target,
            field,
            current,
        });
        cx.notify();
    }

    /// Saves the typed name, unless it is blank or unchanged, or drops it.
    pub fn finish_rename(&mut self, save: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.take() else {
            return;
        };
        let name = renaming.field.read(cx).content.trim().to_string();
        let changed = save && !name.is_empty() && name != renaming.current;
        if let (true, Target::Agent(id)) = (changed, &renaming.target) {
            // An agent's page shows its name too.
            self.rename_agent(id.clone(), name, cx);
        } else if changed {
            let target = renaming.target.clone();
            let rename = move |core: &patchwork_core::Core| match &target {
                Target::Chat(id) => core.rename_chat(id, &name),
                Target::Project(id) => core.rename_project(id, &name),
                Target::Folder(id) => core.rename_folder(id, &name),
                Target::Archived(_) | Target::Many(_) | Target::Agent(_) => Ok(()),
            };
            self.change("rename it", rename, cx);
        }
        if renaming.field.read(cx).focus_handle(cx).is_focused(window) {
            window.focus(&self.focus, cx);
        }
        self.reload(cx);
    }

    /// The name field, sized to the row it sits in.
    pub fn rename_field(&self, target: &Target, cx: &mut Context<Self>) -> Option<AnyElement> {
        let renaming = self.renaming.as_ref().filter(|r| &r.target == target)?;
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
