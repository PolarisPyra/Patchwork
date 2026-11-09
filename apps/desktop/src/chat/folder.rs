//! The project's folder going missing (`prompts/MissingFolderPrompt.tsx`, the folder half of
//! `hooks/useProjectFolder.ts`): once it has been moved or deleted, a strip above the composer
//! says so and offers to point the project elsewhere, and nothing is sent until it is.

use std::sync::Arc;

use gpui::{Context, Div, FontWeight, div, prelude::*, px};

use crate::chat::goal::strip_action;
use crate::ui::theme::{FS_SM, FS_XS, RADIUS_SM, Theme};
use crate::ui::{icon, tooltip};
use crate::workspace::Workspace;

impl Workspace {
    /// Whether the open project's folder was looked for and not found.
    pub fn folder_missing(&self) -> bool {
        let path = self.project().map(|p| p.path.as_str());
        matches!(&self.folder_check, Some((checked, Some(false))) if Some(checked.as_str()) == path)
    }

    /// Looks for the open project's folder off the main thread, once per path.
    fn check_folder(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.project().map(|p| p.path.clone()) else {
            return;
        };
        if self
            .folder_check
            .as_ref()
            .is_some_and(|(checked, _)| *checked == path)
        {
            return;
        }
        self.folder_check = Some((path.clone(), None));
        let task = cx.background_spawn({
            let path = path.clone();
            async move { std::path::Path::new(&path).is_dir() }
        });
        cx.spawn(async move |this, cx| {
            let found = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some((_, seen)) = this.folder_check.as_mut().filter(|(p, _)| *p == path) {
                    *seen = Some(found);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Asks for a folder and points the open project at it.
    fn relocate_project(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.project().map(|p| p.id.clone()) else {
            return;
        };
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        let server = Arc::clone(&self.server);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.first().map(|p| p.to_string_lossy().into_owned()) else {
                return;
            };
            let moved = cx
                .background_spawn(async move { server.relocate_project(&id, &path) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = moved {
                    this.error = Some(e);
                }
                this.settings = this.server.store.settings();
                this.folder_check = None;
                this.reload(cx);
            });
        })
        .detach();
    }
}

pub fn missing_folder(w: &mut Workspace, t: &Theme, cx: &mut Context<Workspace>) -> Option<Div> {
    w.check_folder(cx);
    if !w.folder_missing() {
        return None;
    }
    let path = w.project()?.path.clone();
    Some(
        div()
            .mx(px(8.))
            .mb(px(6.))
            .overflow_hidden()
            .rounded(px(RADIUS_SM))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_panel)
            .text_size(px(FS_SM))
            .shadow_sm()
            .child(
                div()
                    .flex()
                    .h(px(36.))
                    .min_w_0()
                    .items_center()
                    .gap(px(8.))
                    .bg(t.bg_app)
                    .px(px(10.))
                    .child(icon("folder", 13.).flex_none().text_color(t.text_subtle))
                    .child(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text)
                            .child("Folder not found"),
                    )
                    .child(
                        div()
                            .id("missing-folder-path")
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(FS_XS))
                            .text_color(t.text_subtle)
                            .tooltip(tooltip(path.clone()))
                            .child(path),
                    )
                    .child(
                        strip_action("change-folder", "Change folder", t)
                            .on_click(cx.listener(|w, _, _, cx| w.relocate_project(cx))),
                    ),
            ),
    )
}
