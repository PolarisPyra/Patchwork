//! The open menu, drawn at the pointer, and the folder pickers it opens.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{
    AnyElement, KeyDownEvent, Role, SharedString, Window, anchored, deferred, div, prelude::*, px,
};
use crate::ui::theme::{RADIUS, RADIUS_SM, theme};
use crate::ui::{icon, popup_shadow};
use crate::workspace::Workspace;

impl Workspace {
    /// The open menu, drawn over everything at the pointer.
    pub fn context_menu_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.context_menu.as_ref()?;
        let t = *theme(cx);
        let items = self.items(&menu.target);
        let count = items.len();
        let rows = items.into_iter().enumerate().map(|(i, item)| {
            let target = menu.target.clone();
            let act = item.act;
            let highlighted = menu.highlight == Some(i);
            div()
                .id(SharedString::from(format!("context-{i}")))
                .role(Role::MenuItem)
                .aria_label(item.label.clone())
                .flex()
                .h(px(26.))
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .rounded(px(RADIUS_SM))
                .cursor_pointer()
                .whitespace_nowrap()
                .text_color(if item.danger { t.danger } else { t.text })
                .when(highlighted, |row| row.bg(t.bg_hover))
                .hover(|s| s.bg(t.bg_hover))
                .child(icon(item.icon, 14.))
                .child(item.label)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.act(target.clone(), act, window, cx);
                }))
        });
        let body = div()
            .id("context-menu")
            .role(Role::Menu)
            .aria_label("Actions")
            .track_focus(&menu.focus)
            .occlude()
            .flex()
            .flex_col()
            .min_w(px(160.))
            .p(px(4.))
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_elevated)
            .shadow(popup_shadow(&t))
            .text_color(t.text)
            .on_mouse_down_out(
                cx.listener(|this, _, window, cx| this.close_context_menu(window, cx)),
            )
            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                let Some(menu) = this.context_menu.as_mut() else {
                    return;
                };
                let at = menu.highlight;
                match e.keystroke.key.as_str() {
                    "down" => menu.highlight = Some(at.map_or(0, |i| (i + 1) % count)),
                    "up" => {
                        menu.highlight = Some(at.map_or(count - 1, |i| (i + count - 1) % count));
                    }
                    "home" => menu.highlight = Some(0),
                    "end" => menu.highlight = Some(count - 1),
                    "enter" | "space" => {
                        let target = menu.target.clone();
                        if let Some(item) = at.and_then(|i| this.items(&target).into_iter().nth(i))
                        {
                            this.act(target, item.act, window, cx);
                        }
                    }
                    "escape" => this.close_context_menu(window, cx),
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .children(rows);
        Some(
            deferred(
                anchored()
                    .position(menu.at)
                    .snap_to_window_with_margin(px(8.))
                    .child(body),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

impl Workspace {
    /// Makes a worktree of the project, as a project of its own, and starts a task in it.
    pub(crate) fn new_task_in_worktree(
        &mut self,
        project_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let server = Arc::clone(&self.server);
        let made = cx.background_spawn(async move { server.create_worktree(&project_id) });
        cx.spawn_in(window, async move |this, cx| {
            let made = made.await;
            let _ = this.update_in(cx, |this, window, cx| match made {
                Ok(project) => {
                    this.settings = this.server.store.settings();
                    this.new_thread_in(Some(project.id), None, window, cx);
                }
                Err(e) => {
                    this.error = Some(e.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Asks for the folder a moved project is in now, and points the project there.
    pub(super) fn change_project_folder(&mut self, project_id: String, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.first().map(|p| p.to_string_lossy().into_owned()) else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                match this.server.relocate_project(&project_id, &path) {
                    Ok(()) => this.settings = this.server.store.settings(),
                    Err(e) => this.error = Some(e.to_string()),
                }
                this.reload(cx);
            });
        })
        .detach();
    }
}
