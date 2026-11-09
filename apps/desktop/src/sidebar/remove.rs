//! Removing rows from the task tree (`useThreadRemoval.ts`): archiving chats and tasks, removing
//! projects and folders once asked, and deleting archived ones for good.

use gpui::{Context, PromptButton, PromptLevel, Window};
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
        targets: Vec<Target>,
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
                    .map_or("this folder".to_string(), |f| f.name.clone());
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
                    .map_or("this project".to_string(), |p| p.name.clone());
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
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                // Projects go first: the server deletes their tasks with them.
                for id in &projects {
                    match this.server.remove_project(id) {
                        Ok(()) => {
                            if this
                                .thread
                                .as_ref()
                                .is_some_and(|t| t.project_id.as_ref() == Some(id))
                            {
                                this.thread = None;
                            }
                        }
                        Err(e) => log::warn!("could not remove the project: {e}"),
                    }
                }
                this.settings = this.server.store.settings();
                for id in &folders {
                    if let Err(e) = this.server.remove_folder(id) {
                        log::warn!("could not remove the folder: {e}");
                    }
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
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                for id in &ids {
                    this.server.delete_thread(id);
                }
                if this.thread.as_ref().is_some_and(|t| ids.contains(&t.id)) {
                    this.thread = None;
                }
                this.reload(cx);
            });
        })
        .detach();
    }
}
