//! What a row's menu offers and what each of its items does.

// Rust guideline compliant 2026-10-09

use super::types::ContextMenu;
use super::types::Act;
use super::types::Item;
use super::types::Target;
use std::sync::Arc;
use gpui::{MouseDownEvent, SharedString, Window, prelude::*};
use patchwork_server::threads::Thread;
use crate::workspace::{GranularScope, Workspace};

impl Workspace {
    pub(in crate::sidebar) fn thread_by_id(&self, id: &str) -> Option<&Thread> {
        self.threads.iter().find(|t| t.id == id).map(Arc::as_ref)
    }

    pub(super) fn items(&self, target: &Target) -> Vec<Item> {
        let item = |label: &'static str, icon, act| Item {
            label: label.into(),
            icon,
            danger: false,
            act,
        };
        let delete = Item {
            danger: true,
            ..item("Delete permanently", "trash", Act::Remove)
        };
        match target {
            Target::Archived(_) => return vec![delete],
            Target::Many(targets) => return self.many_items(targets),
            Target::Thread(id)
                if self
                    .thread_by_id(id)
                    .is_some_and(|t| t.archived_at.is_some()) =>
            {
                return vec![
                    item("Restore", "arrow-counter-clockwise", Act::Restore),
                    delete,
                ];
            }
            _ => {}
        }
        let mut items = vec![item("Rename", "pencil-simple", Act::Rename)];
        match target {
            Target::Archived(_) | Target::Many(_) => {}
            Target::Thread(id) => {
                if let Some(thread) = self.thread_by_id(id) {
                    items.push(if thread.favorite {
                        item("Remove from favorites", "star", Act::Favorite(false))
                    } else {
                        item("Add to favorites", "star", Act::Favorite(true))
                    });
                    if thread.project_id.is_some() {
                        items.push(if thread.pinned {
                            item("Unpin", "push-pin-slash", Act::Pin(false))
                        } else {
                            item("Pin", "push-pin", Act::Pin(true))
                        });
                    }
                }
                items.push(item("Background…", "image", Act::Background));
                if self.settings.granular_chats {
                    items.push(item("Granular settings…", "gauge", Act::Granular));
                }
                items.push(item("Export to…", "export", Act::Export));
                if self.settings.logging.thread_menu {
                    items.push(item("Open logs", "file-text", Act::OpenLogs));
                }
                items.push(item("Archive", "archive", Act::Remove));
            }
            Target::Project(_) => {
                items.push(item(
                    "New task in a worktree",
                    "git-branch",
                    Act::WorktreeTask,
                ));
                if self.settings.granular_chats {
                    items.push(item("Granular settings…", "gauge", Act::Granular));
                }
                items.push(item("Change folder…", "folder", Act::ChangeFolder));
                items.push(item("Open folder", "folder-open", Act::OpenFolder));
                items.push(Item {
                    danger: true,
                    ..item("Remove", "trash", Act::Remove)
                });
            }
            Target::Folder(_) => items.push(Item {
                danger: true,
                ..item("Remove", "trash", Act::Remove)
            }),
        }
        items
    }

    /// The items for rows picked together: favorites and pins when they are all chats or tasks,
    /// and the one that archives or removes them all.
    fn many_items(&self, targets: &[Target]) -> Vec<Item> {
        let threads: Vec<&Thread> = targets
            .iter()
            .filter_map(|t| match t {
                Target::Thread(id) => self.thread_by_id(id),
                _ => None,
            })
            .collect();
        let archiving = threads.len() == targets.len();
        let item = |label: &str, icon, act| Item {
            label: SharedString::from(label.to_string()),
            icon,
            danger: false,
            act,
        };
        let mut items = Vec::new();
        if archiving {
            // Some not yet a favorite, or not yet pinned: the item adds them all; otherwise it
            // takes them all off.
            let favorite = threads.iter().any(|t| !t.favorite);
            items.push(if favorite {
                item("Add to favorites", "star", Act::Favorite(true))
            } else {
                item("Remove from favorites", "star", Act::Favorite(false))
            });
            if threads.iter().all(|t| t.project_id.is_some()) {
                items.push(if threads.iter().any(|t| !t.pinned) {
                    item("Pin", "push-pin", Act::Pin(true))
                } else {
                    item("Unpin", "push-pin-slash", Act::Pin(false))
                });
            }
        }
        let n = targets.len();
        items.push(if archiving {
            item(&format!("Archive {n} items"), "archive", Act::Remove)
        } else {
            Item {
                danger: true,
                ..item(&format!("Remove {n} items"), "trash", Act::Remove)
            }
        });
        items
    }

    /// Opens the menu for `target` where the pointer pressed.
    pub fn open_context_menu(
        &mut self,
        target: Target,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        // The press otherwise goes on to focus the window under the row, and the menu, without
        // the focus, hears none of its keys.
        window.prevent_default();
        self.context_menu = Some(ContextMenu {
            at: event.position,
            target,
            focus,
            highlight: None,
        });
        cx.notify();
    }

    pub(super) fn close_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// Runs the Delete key: on the row whose menu is open, else the picked rows, else the chat row
    /// just clicked while it is still the open one. A chat or task is archived, and an archived one
    /// is deleted for good once asked.
    pub(crate) fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = if let Some(menu) = &self.context_menu {
            menu.target.clone()
        } else if !self.picking.rows.is_empty() {
            Target::Many(self.picking.rows.clone())
        } else if let Some(Target::Thread(id)) = &self.picking.clicked
            && self
                .thread
                .as_ref()
                .is_some_and(|t| &t.id == id && t.parent.is_none())
        {
            Target::Thread(id.clone())
        } else {
            return;
        };
        self.act(target, Act::Remove, window, cx);
    }

    pub(super) fn act(
        &mut self,
        target: Target,
        act: Act,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_context_menu(window, cx);
        let done: Result<(), patchwork_server::Error> = match (&target, act) {
            (_, Act::Rename) => return self.start_rename(target, window, cx),
            (Target::Thread(id), Act::Favorite(_) | Act::Pin(_) | Act::Restore) => {
                let id = id.clone();
                let change = move |server: &patchwork_server::Server| match act {
                    Act::Favorite(on) => server.set_thread_favorite(&id, on),
                    Act::Pin(on) => server.set_thread_pinned(&id, on),
                    _ => server.set_thread_archived(&id, false),
                };
                return self.change("change the task list", change, cx);
            }
            (Target::Thread(id), Act::Remove)
                if self
                    .thread_by_id(id)
                    .is_some_and(|t| t.archived_at.is_some()) =>
            {
                return self.confirm_delete(vec![id.clone()], window, cx);
            }
            (Target::Archived(ids), Act::Remove) => {
                return self.confirm_delete(ids.clone(), window, cx);
            }
            (Target::Thread(id), Act::Remove) => return self.archive(id, cx),
            (Target::Thread(id), Act::Background) => {
                return self.open_background_dialog(id, window, cx);
            }
            (Target::Thread(id), Act::Granular) => {
                let scope = GranularScope::Chat(id.clone());
                return self.open_granular_dialog(scope, None, window, cx);
            }
            (Target::Project(id), Act::Granular) => {
                let scope = GranularScope::Project(id.clone());
                return self.open_granular_dialog(scope, None, window, cx);
            }
            (Target::Thread(id), Act::Export) => return self.export_thread(id, cx),
            (Target::Thread(id), Act::OpenLogs) => {
                let (server, id) = (Arc::clone(&self.server), id.clone());
                let report = cx.background_spawn(async move { server.thread_report(&id) });
                cx.spawn(async move |_, cx| match report.await {
                    Ok(path) => cx.update(|cx| cx.reveal_path(&path)),
                    Err(e) => {
                        log::warn!(error:% = e; "could not show the logs of the chat: {{error}}");
                    }
                })
                .detach();
                return;
            }
            (Target::Project(id), Act::WorktreeTask) => {
                return self.new_task_in_worktree(id.clone(), window, cx);
            }
            (Target::Project(id), Act::ChangeFolder) => {
                return self.change_project_folder(id.clone(), cx);
            }
            (Target::Project(id), Act::OpenFolder) => {
                let Some(project) = self.settings.projects.iter().find(|p| &p.id == id) else {
                    return;
                };
                cx.reveal_path(std::path::Path::new(&project.path));
                Ok(())
            }
            (Target::Project(_) | Target::Folder(_), Act::Remove) => {
                return self.remove_targets(&[target], window, cx);
            }
            (Target::Many(targets), Act::Remove) => {
                return self.remove_targets(targets, window, cx);
            }
            (Target::Many(targets), Act::Favorite(_) | Act::Pin(_)) => {
                let ids: Vec<String> = targets
                    .iter()
                    .filter_map(|t| match t {
                        Target::Thread(id) => Some(id.clone()),
                        _ => None,
                    })
                    .collect();
                let change = move |server: &patchwork_server::Server| {
                    ids.iter().try_for_each(|id| match act {
                        Act::Favorite(on) => server.set_thread_favorite(id, on),
                        _ => server.set_thread_pinned(id, matches!(act, Act::Pin(true))),
                    })
                };
                return self.change("change the task list", change, cx);
            }
            _ => Ok(()),
        };
        if let Err(e) = done {
            log::warn!(error:% = e; "could not change the task list: {{error}}");
        }
        self.reload(cx);
    }
}
