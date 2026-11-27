//! What a row's menu offers and what each of its items does.

// Rust guideline compliant 2026-10-09

use super::target::ContextMenu;
use super::target::Act;
use super::target::Item;
use super::target::Target;
use std::sync::Arc;
use gpui::{MouseDownEvent, SharedString, Window, prelude::*};
use patchwork_backend::chats::Chat;
use crate::chat::panes::{Dir, PaneId, Preset, Zone};
use crate::workspace::Workspace;
use patchwork_desktop_dialogs::granular::GranularScope;
use patchwork_ui_theme::IconName;

impl Workspace {
    pub(in crate::sidebar) fn chat_by_id(&self, id: &str) -> Option<&Chat> {
        self.chats.iter().find(|t| t.id == id).map(Arc::as_ref)
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
            ..item("Delete permanently", IconName::Trash, Act::Remove)
        };
        match target {
            Target::Archived(_) => return vec![delete],
            Target::Many(targets) => return self.many_items(targets),
            Target::Tabs(project) => return self.tab_items(project.as_deref()),
            Target::Layouts => {
                let named = self.named_layouts.iter().map(|(name, _)| Item {
                    label: name.clone().into(),
                    ..item("", IconName::Star, Act::Layout(name.clone()))
                });
                return Preset::ALL
                    .into_iter()
                    .map(|p| item(p.label(), p.icon(), Act::Preset(p)))
                    .chain(named)
                    .collect();
            }
            Target::Pane(pane) => return self.pane_items(*pane),
            Target::Chat(id) if self.chat_by_id(id).is_some_and(|t| t.archived_at.is_some()) => {
                return vec![
                    item("Restore", IconName::ArrowCounterClockwise, Act::Restore),
                    delete,
                ];
            }
            Target::Agent(id)
                if self
                    .agents
                    .iter()
                    .any(|b| &b.id == id && b.archived_at.is_some()) =>
            {
                return vec![
                    item("Restore", IconName::ArrowCounterClockwise, Act::Restore),
                    delete,
                ];
            }
            _ => {}
        }
        let mut items = vec![item("Rename", IconName::PencilSimple, Act::Rename)];
        match target {
            Target::Archived(_)
            | Target::Many(_)
            | Target::Tabs(_)
            | Target::Layouts
            | Target::Pane(_) => {}
            Target::Agent(_) => {
                items.push(item("Details", IconName::Robot, Act::Details));
                items.push(item("Background…", IconName::Image, Act::Background));
                items.push(Item {
                    danger: true,
                    ..item("Delete", IconName::Trash, Act::Remove)
                });
            }
            Target::Chat(id) => {
                if self.has_tab(id) {
                    items.insert(0, item("Close tab", IconName::X, Act::CloseTab));
                    let (others, right) = self.tabs_beside(id);
                    let mut at = 1;
                    if others {
                        items.insert(
                            at,
                            item("Close other tabs", IconName::XCircle, Act::CloseOthers),
                        );
                        at += 1;
                    }
                    if right {
                        items.insert(
                            at,
                            item(
                                "Close tabs to the right",
                                IconName::XCircle,
                                Act::CloseRight,
                            ),
                        );
                    }
                } else {
                    items.insert(
                        0,
                        item("Open in new tab", IconName::Plus, Act::OpenInNewTab),
                    );
                }
                if self.can_split(id) {
                    items.insert(
                        1,
                        item(
                            "Split right",
                            IconName::SquareSplitHorizontal,
                            Act::Split(Zone::Right),
                        ),
                    );
                    items.insert(
                        2,
                        item(
                            "Split down",
                            IconName::SquareSplitVertical,
                            Act::Split(Zone::Bottom),
                        ),
                    );
                }
                if let Some(chat) = self.chat_by_id(id) {
                    items.push(if chat.favorite {
                        item(
                            "Remove from favorites",
                            IconName::Star,
                            Act::Favorite(false),
                        )
                    } else {
                        item("Add to favorites", IconName::Star, Act::Favorite(true))
                    });
                    if chat.project_id.is_some() {
                        items.push(if chat.pinned {
                            item("Unpin", IconName::PushPinSlash, Act::Pin(false))
                        } else {
                            item("Pin", IconName::PushPin, Act::Pin(true))
                        });
                    }
                }
                if self.settings.granular_chats {
                    items.push(item("Granular settings…", IconName::Gauge, Act::Granular));
                }
                items.extend(self.chat_app_items(id));
                items.push(item("Export to…", IconName::Export, Act::Export));
                if self.settings.logging.chat_menu {
                    items.push(item("Open logs", IconName::FileText, Act::OpenLogs));
                }
                items.push(item("Archive", IconName::Archive, Act::Remove));
            }
            Target::Project(id) => {
                // A project on an SSH host has no folder here to show.
                let remote = self
                    .settings
                    .projects
                    .iter()
                    .any(|p| &p.id == id && patchwork_platform::ssh::is_remote(&p.path));
                items.push(item(
                    "New task in a worktree",
                    IconName::Folders,
                    Act::WorktreeTask,
                ));
                if !remote && !self.settings.worktrees.disabled {
                    let in_place = self.settings.edit_in_place.iter().any(|p| p == id);
                    items.push(if in_place {
                        item(
                            "Give tasks their own worktree",
                            IconName::Folders,
                            Act::EditInPlace(false),
                        )
                    } else {
                        item(
                            "Edit tasks in the project folder",
                            IconName::Folder,
                            Act::EditInPlace(true),
                        )
                    });
                }
                if self.settings.granular_chats {
                    items.push(item("Granular settings…", IconName::Gauge, Act::Granular));
                }
                items.push(item("Change folder…", IconName::Folder, Act::ChangeFolder));
                if !remote {
                    items.push(item("Open folder", IconName::FolderOpen, Act::OpenFolder));
                }
                items.push(Item {
                    danger: true,
                    ..item("Remove", IconName::Trash, Act::Remove)
                });
            }
            Target::Folder(_) => items.push(Item {
                danger: true,
                ..item("Remove", IconName::Trash, Act::Remove)
            }),
        }
        items
    }

    /// The items that stop a chat app's conversation talking to the chat `id`.
    fn chat_app_items(&self, id: &str) -> Vec<Item> {
        let bound = self
            .chat_apps
            .get(id)
            .map_or(&[][..], |apps| apps.conversations.as_slice());
        bound
            .iter()
            // A conversation whose agent only works with this chat talks to another.
            .filter(|b| b.chat_id == id)
            .map(|b| Item {
                label: format!("Stop continuing on {}", b.platform.label()).into(),
                icon: IconName::X,
                danger: false,
                act: Act::StopOn(b.platform, b.conversation.clone()),
            })
            .collect()
    }

    /// The items for rows picked together: favorites and pins when they are all chats or tasks,
    /// and the one that archives or removes them all.
    fn many_items(&self, targets: &[Target]) -> Vec<Item> {
        let chats: Vec<&Chat> = targets
            .iter()
            .filter_map(|t| match t {
                Target::Chat(id) => self.chat_by_id(id),
                _ => None,
            })
            .collect();
        let archiving = chats.len() == targets.len();
        let item = |label: &str, icon, act| Item {
            label: SharedString::from(label.to_string()),
            icon,
            danger: false,
            act,
        };
        let mut items = Vec::new();
        let project = self.tabs_project();
        if archiving
            && (2..=4).contains(&chats.len())
            && chats.iter().all(|c| c.project_id == project)
        {
            items.push(item(
                "Open side by side",
                IconName::SquareSplitHorizontal,
                Act::SideBySide,
            ));
        }
        if archiving {
            // Some not yet a favorite, or not yet pinned: the item adds them all; otherwise it
            // takes them all off.
            let favorite = chats.iter().any(|t| !t.favorite);
            items.push(if favorite {
                item("Add to favorites", IconName::Star, Act::Favorite(true))
            } else {
                item(
                    "Remove from favorites",
                    IconName::Star,
                    Act::Favorite(false),
                )
            });
            if chats.iter().all(|t| t.project_id.is_some()) {
                items.push(if chats.iter().any(|t| !t.pinned) {
                    item("Pin", IconName::PushPin, Act::Pin(true))
                } else {
                    item("Unpin", IconName::PushPinSlash, Act::Pin(false))
                });
            }
        }
        let n = targets.len();
        items.push(if archiving {
            item(
                &format!("Archive {n} items"),
                IconName::Archive,
                Act::Remove,
            )
        } else {
            Item {
                danger: true,
                ..item(&format!("Remove {n} items"), IconName::Trash, Act::Remove)
            }
        });
        items
    }

    /// A tab strip's menu: split the pane, show it alone, close it.
    fn pane_items(&self, pane: PaneId) -> Vec<Item> {
        let item = |label: &'static str, icon, act| Item {
            label: label.into(),
            icon,
            danger: false,
            act,
        };
        let layout = self.layouts.get(&self.tabs_project());
        let several = layout.is_some_and(|l| l.panes.len() > 1);
        let maximized = layout.is_some_and(|l| l.maximized.is_some());
        let mut items = vec![
            item(
                "Split right",
                IconName::SquareSplitHorizontal,
                Act::SplitPane(Zone::Right),
            ),
            item(
                "Split down",
                IconName::SquareSplitVertical,
                Act::SplitPane(Zone::Bottom),
            ),
        ];
        if maximized {
            items.push(item(
                "Show all panes",
                IconName::ArrowsInSimple,
                Act::Maximize,
            ));
        } else if several && layout.is_some_and(|l| l.focused == pane) {
            items.push(item(
                "Maximize pane",
                IconName::ArrowsOutSimple,
                Act::Maximize,
            ));
        }
        if several {
            items.push(if self.broadcasting {
                item(
                    "Stop sending to every pane",
                    IconName::PaperPlaneRight,
                    Act::Broadcast,
                )
            } else {
                item(
                    "Send messages to every pane",
                    IconName::PaperPlaneRight,
                    Act::Broadcast,
                )
            });
            if let Some(layout) = layout {
                for (label, dir) in [
                    ("Swap with pane left", Dir::Left),
                    ("Swap with pane right", Dir::Right),
                    ("Swap with pane above", Dir::Up),
                    ("Swap with pane below", Dir::Down),
                ] {
                    if let Some(other) = layout.neighbour(pane, dir) {
                        items.push(item(label, IconName::ArrowsLeftRight, Act::SwapPane(other)));
                    }
                }
            }
            items.push(item("Close pane", IconName::X, Act::ClosePane));
        }
        items
    }

    /// The tab add button's menu: a new task, then each of the project's tasks with no tab.
    fn tab_items(&self, project: Option<&str>) -> Vec<Item> {
        let new = Item {
            label: if project.is_some() {
                "New task"
            } else {
                "New chat"
            }
            .into(),
            icon: IconName::Plus,
            danger: false,
            act: Act::NewTaskTab,
        };
        let tasks = self.untabbed_tasks(project).into_iter().map(|c| {
            let title = patchwork_ui_markdown::display_title(&c.title);
            Item {
                label: if title.trim().is_empty() {
                    "New chat".into()
                } else {
                    title.into()
                },
                icon: IconName::ChatCircle,
                danger: false,
                act: Act::OpenTab(c.id.clone()),
            }
        });
        std::iter::once(new).chain(tasks).collect()
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
        } else if let Some(Target::Chat(id)) = &self.picking.clicked
            && self
                .chat
                .as_ref()
                .is_some_and(|t| &t.id == id && t.parent.is_none())
        {
            Target::Chat(id.clone())
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
        match (&target, &act) {
            (_, Act::Rename) => return self.start_rename(target, window, cx),
            (Target::Chat(id), Act::CloseTab) => return self.close_tab(id, cx),
            (Target::Chat(id), Act::CloseOthers) => return self.close_tabs_beside(id, false, cx),
            (Target::Chat(id), Act::CloseRight) => return self.close_tabs_beside(id, true, cx),
            (Target::Pane(pane), Act::SplitPane(zone)) => {
                return self.split_pane_at(*pane, *zone, cx);
            }
            (Target::Pane(_), Act::Maximize) => return self.toggle_maximized(cx),
            (Target::Pane(pane), Act::ClosePane) => return self.close_pane(*pane, cx),
            (Target::Pane(pane), Act::SwapPane(other)) => {
                return self.swap_panes(*pane, *other, cx);
            }
            (Target::Pane(_), Act::Broadcast) => return self.toggle_broadcast(cx),
            (Target::Many(targets), Act::SideBySide) => {
                let ids: Vec<String> = targets
                    .iter()
                    .filter_map(|t| match t {
                        Target::Chat(id) => Some(id.clone()),
                        _ => None,
                    })
                    .collect();
                return self.open_side_by_side(&ids, cx);
            }
            (Target::Tabs(project), Act::NewTaskTab) => {
                return self.new_task_tab(project.clone(), window, cx);
            }
            (Target::Layouts, Act::Preset(preset)) => return self.apply_preset(*preset, cx),
            (Target::Layouts, Act::Layout(name)) => return self.apply_named_layout(name, cx),
            (Target::Chat(id), Act::Split(zone)) => return self.open_in_split(id, *zone, cx),
            (Target::Chat(id), Act::OpenInNewTab) | (_, Act::OpenTab(id)) => {
                return self.open_in_new_tab(id, cx);
            }
            (Target::Chat(id), Act::Favorite(_) | Act::Pin(_) | Act::Restore) => {
                let id = id.clone();
                let change = move |core: &Arc<patchwork_backend::Core>| match act {
                    Act::Favorite(on) => core.set_chat_favorite(&id, on),
                    Act::Pin(on) => core.set_chat_pinned(&id, on),
                    _ => core.chats().archive(&id, false),
                };
                return self.change("change the task list", change, cx);
            }
            (Target::Chat(id), Act::Remove)
                if self.chat_by_id(id).is_some_and(|t| t.archived_at.is_some()) =>
            {
                return self.confirm_delete(vec![id.clone()], window, cx);
            }
            (Target::Archived(ids), Act::Remove) => {
                return self.confirm_delete(ids.clone(), window, cx);
            }
            (Target::Chat(id), Act::Remove) => return self.archive(id, cx),
            (Target::Agent(id), Act::Details) => return self.open_agent_details(id, cx),
            (Target::Agent(id), Act::Restore) => return self.set_agent_archived(id, false, cx),
            (Target::Agent(id), Act::Background) => {
                return self.open_agent_background_dialog(id, window, cx);
            }
            (Target::Chat(id), Act::Granular) => {
                let scope = GranularScope::Chat(id.clone());
                return self.open_granular_dialog(scope, None, window, cx);
            }
            (Target::Project(id), Act::Granular) => {
                let scope = GranularScope::Project(id.clone());
                return self.open_granular_dialog(scope, None, window, cx);
            }
            (Target::Chat(id), Act::Export) => return self.export_chat(id, cx),
            (Target::Chat(_), Act::StopOn(platform, chat)) => {
                let (platform, chat) = (*platform, chat.clone());
                let change = move |core: &Arc<patchwork_backend::Core>| {
                    core.stop_continuing_on_chat(platform, &chat);
                    Ok(())
                };
                return self.change("stop continuing the chat on a chat app", change, cx);
            }
            (Target::Chat(id), Act::OpenLogs) => {
                let (core, id) = (Arc::clone(&self.core), id.clone());
                let report = cx.background_spawn(async move { core.chat_report(&id) });
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
            (Target::Project(id), Act::EditInPlace(on)) => {
                let (id, on) = (id.clone(), *on);
                let change =
                    move |core: &Arc<patchwork_backend::Core>| core.set_edits_in_place(&id, on);
                return self.change("change where the project's tasks work", change, cx);
            }
            (Target::Project(id), Act::ChangeFolder) => {
                return self.change_project_folder(id.clone(), cx);
            }
            (Target::Project(id), Act::OpenFolder) => {
                let Some(project) = self.settings.projects.iter().find(|p| &p.id == id) else {
                    return;
                };
                cx.reveal_path(std::path::Path::new(&project.path));
            }
            (Target::Project(_) | Target::Folder(_), Act::Remove) => {
                return self.remove_targets(&[target], window, cx);
            }
            (Target::Agent(id), Act::Remove) => return self.delete_agent(id, window, cx),
            (Target::Many(targets), Act::Remove) => {
                return self.remove_targets(targets, window, cx);
            }
            (Target::Many(targets), Act::Favorite(_) | Act::Pin(_)) => {
                let ids: Vec<String> = targets
                    .iter()
                    .filter_map(|t| match t {
                        Target::Chat(id) => Some(id.clone()),
                        _ => None,
                    })
                    .collect();
                let change = move |core: &Arc<patchwork_backend::Core>| {
                    ids.iter().try_for_each(|id| match act {
                        Act::Favorite(on) => core.set_chat_favorite(id, on),
                        _ => core.set_chat_pinned(id, matches!(act, Act::Pin(true))),
                    })
                };
                return self.change("change the task list", change, cx);
            }
            _ => {}
        }
        self.reload(cx);
    }
}
