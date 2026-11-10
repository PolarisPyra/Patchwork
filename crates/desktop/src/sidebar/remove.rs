//! Removing rows from the task tree: archiving chats and tasks, removing
//! projects and folders once asked, and deleting archived ones for good.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{AppContext as _, Context, PromptButton, PromptLevel, Window};
use patchwork_server::threads::Thread;

use super::menu::Target;
use crate::ui::display_title;
use crate::workspace::Workspace;

impl Workspace {
    /// Archives the chats and tasks among `targets`, and asks once before removing projects, which
    /// take their tasks, and folders, whose tasks move up. Folders and tasks inside a removed
    /// project are left out, as the project takes them too.
    pub(super) fn remove_targets(
        &mut self,
        targets: &[Target],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let projects: Vec<String> = targets
            .iter()
            .filter_map(|t| match t {
                Target::Project(id) => Some(id.clone()),
                _ => None,
            })
            .collect();
        let folders: Vec<String> = targets
            .iter()
            .filter_map(|t| match t {
                Target::Folder(id) => self.folders.iter().find(|f| &f.id == id),
                _ => None,
            })
            .filter(|f| !projects.contains(&f.project_id))
            .map(|f| f.id.clone())
            .collect();
        let threads: Vec<String> = targets
            .iter()
            .filter_map(|t| match t {
                Target::Thread(id) => self.thread_by_id(id),
                _ => None,
            })
            .filter(|t| t.project_id.as_ref().is_none_or(|p| !projects.contains(p)))
            .map(|t| t.id.clone())
            .collect();
        self.picking.rows.clear();
        let (title, message) = match (projects.as_slice(), folders.as_slice()) {
            ([], []) => {
                for id in &threads {
                    self.archive(id, cx);
                }
                return;
            }
            ([], [id]) => {
                let name = self
                    .folders
                    .iter()
                    .find(|f| &f.id == id)
                    .map_or_else(|| "this folder".to_string(), |f| f.name.clone());
                (
                    "Remove folder",
                    format!("Remove {name}? Its tasks move to the top of the project."),
                )
            }
            ([id], []) => {
                let name = self
                    .settings
                    .projects
                    .iter()
                    .find(|p| &p.id == id)
                    .map_or_else(|| "this project".to_string(), |p| p.name.clone());
                ("Remove project", format!("Remove {name} and its tasks?"))
            }
            _ => (
                "Remove",
                format!(
                    "Remove {} items? Removing a project also removes its tasks.",
                    projects.len() + folders.len()
                ),
            ),
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            title,
            Some(&message),
            &["Remove", "Cancel"],
            cx,
        );
        let server = Arc::clone(&self.server);
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            // Removing a worktree runs git, which is not for the UI thread.
            let (removed, failed) = cx
                .background_spawn(async move {
                    let mut removed = Vec::new();
                    let mut failed = None;
                    // Projects go first: the server deletes their tasks with them.
                    for id in projects {
                        match server.remove_project(&id) {
                            Ok(()) => removed.push(id),
                            Err(e) => {
                                log::warn!(error:% = e; "could not remove the project: {{error}}");
                                failed.get_or_insert_with(|| e.to_string());
                            }
                        }
                    }
                    for id in &folders {
                        if let Err(e) = server.remove_folder(id) {
                            log::warn!(error:% = e; "could not remove the folder: {{error}}");
                            failed.get_or_insert_with(|| e.to_string());
                        }
                    }
                    (removed, failed)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let open = this.thread.as_ref().and_then(|t| t.project_id.as_ref());
                if open.is_some_and(|id| removed.contains(id)) {
                    this.thread = None;
                }
                this.settings = this.server.store.settings();
                if failed.is_some() {
                    this.error = failed;
                }
                for id in &threads {
                    this.archive(id, cx);
                }
                this.reload(cx);
            });
        })
        .detach();
    }

    /// Asks before deleting archived chats or tasks for good, naming what goes.
    pub(super) fn confirm_delete(
        &mut self,
        ids: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (title, message) = match ids.as_slice() {
            [] => return,
            [id] => {
                let name = self
                    .thread_by_id(id)
                    .map_or(String::new(), |t| display_title(&t.title));
                (
                    format!("Delete {name}?"),
                    "Its messages cannot be recovered.",
                )
            }
            _ => {
                let picked: Vec<&Thread> = self
                    .threads
                    .iter()
                    .filter(|t| ids.contains(&t.id))
                    .collect();
                let chats = picked.iter().filter(|t| t.project_id.is_none()).count();
                let tasks = picked.len() - chats;
                let count = |n: usize, one: &str, many: &str| match n {
                    0 => None,
                    1 => Some(format!("1 {one}")),
                    n => Some(format!("{n} {many}")),
                };
                let counts: Vec<String> =
                    [count(chats, "chat", "chats"), count(tasks, "task", "tasks")]
                        .into_iter()
                        .flatten()
                        .collect();
                (
                    format!("Delete {}?", counts.join(" and ")),
                    "Their messages cannot be recovered.",
                )
            }
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &title,
            Some(message),
            &[PromptButton::ok("Delete"), PromptButton::cancel("Keep")],
            cx,
        );
        let server = Arc::clone(&self.server);
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            // Deleting a task drops its checkpoints with git, which is not for the UI thread.
            let ids = cx
                .background_spawn(async move {
                    for id in &ids {
                        server.delete_thread(id);
                    }
                    ids
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.thread.as_ref().is_some_and(|t| ids.contains(&t.id)) {
                    this.thread = None;
                }
                this.reload(cx);
            });
        })
        .detach();
    }
}
