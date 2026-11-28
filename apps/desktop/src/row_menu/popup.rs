//! The open menu, drawn at the pointer, and the folder pickers it opens.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_chat::prelude::*;
use patchwork_client::AppState;
use std::sync::Arc;
use gpui::{AnyElement, Window, prelude::*};
use patchwork_design_components::menu::Menu;
use crate::window::Workspace;

impl Workspace {
    /// The open menu, drawn over everything at the pointer.
    pub fn context_menu_view(&self, _cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.context_menu.as_ref()?;
        Some(Menu::view(&open.menu, open.at))
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
        let core = Arc::clone(&self.core);
        let made = cx.background_spawn(async move { core.create_worktree(&project_id) });
        cx.spawn_in(window, async move |this, cx| {
            let made = made.await;
            let _ = this.update_in(cx, |this, window, cx| match made {
                Ok(project) => {
                    AppState::update(cx, AppState::reload_settings);
                    this.new_chat_in(Some(project.id), None, window, cx);
                }
                Err(e) => {
                    AppState::set_error(cx, Some(e.to_string()));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Asks for the folder a moved project is in now, and points the project there.
    pub(super) fn change_project_folder(&mut self, project_id: String, cx: &mut Context<Self>) {
        patchwork_design_tasks::pick_paths(
            cx,
            gpui::PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: None,
            },
            move |this, paths, cx| {
                let Some(path) = paths.first().map(|p| p.to_string_lossy().into_owned()) else {
                    return;
                };

                match this.core.relocate_project(&project_id, &path) {
                    Ok(()) => AppState::update(cx, AppState::reload_settings),
                    Err(e) => AppState::set_error(cx, Some(e.to_string())),
                }
                this.reload(cx);
            },
        );
    }
}
