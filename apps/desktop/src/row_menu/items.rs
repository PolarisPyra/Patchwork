//! What a row's menu offers and what each of its items does.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_chat::prelude::*;
use patchwork_client::AppState;
use patchwork_client::local::gateway::CoreHandoff;
use super::target::ContextMenu;
use super::target::Act;
use super::target::Item;
use super::target::Target;
use super::target::grouped;
use patchwork_design_components::menu::{Dismissed, Menu, MenuItem};
use patchwork_views_actions::{
    DeleteChat, MaximizePane, SplitPaneDown, SplitPaneRight, SwapPaneDown, SwapPaneLeft,
    SwapPaneRight, SwapPaneUp,
};
use std::sync::Arc;
use gpui::{Pixels, Point, SharedString, Window, prelude::*};
use patchwork_core::chats::Chat;
use patchwork_desktop_chat::panes::{Dir, PaneId, Preset, Zone};
use crate::window::Workspace;
use patchwork_desktop_dialogs::granular::GranularScope;
use patchwork_design_icons::IconName;

impl Workspace {
    pub(crate) fn chat_by_id<'a>(&'a self, id: &str, cx: &'a gpui::App) -> Option<&'a Chat> {
        AppState::get(cx)
            .chats
            .iter()
            .find(|t| t.id == id)
            .map(Arc::as_ref)
    }

    pub(super) fn items(&self, target: &Target, cx: &gpui::App) -> Vec<Item> {
        let item = Item::new;
        let delete = || item("Delete permanently", IconName::Trash, Act::Remove).danger();
        let restore = || item("Restore", IconName::ArrowCounterClockwise, Act::Restore);
        let state = AppState::get(cx);
        match target {
            Target::Archived(_) => vec![delete()],
            Target::Many(targets) => self.many_items(targets, cx),
            Target::Tabs(project) => self.tab_items(project.as_deref(), cx),
            Target::Layouts => {
                let presets = Preset::ALL
                    .into_iter()
                    .map(|p| item(p.label(), p.icon(), Act::Preset(p)))
                    .collect();
                let named = self
                    .named_layouts
                    .iter()
                    .map(|(name, _)| {
                        Item::new(name.clone(), IconName::Star, Act::Layout(name.clone()))
                    })
                    .collect();
                grouped([presets, named])
            }
            Target::Pane(pane) => self.pane_items(*pane, cx),
            Target::Chat(id)
                if self
                    .chat_by_id(id, cx)
                    .is_some_and(|t| t.archived_at.is_some()) =>
            {
                grouped([vec![restore()], vec![delete()]])
            }
            Target::Agent(id)
                if state
                    .agents
                    .iter()
                    .any(|b| &b.id == id && b.archived_at.is_some()) =>
            {
                grouped([vec![restore()], vec![delete()]])
            }
            Target::Chat(id) => self.chat_items(id, cx),
            Target::Agent(_) => grouped([
                vec![
                    item("Rename", IconName::PencilSimple, Act::Rename),
                    item("Details", IconName::Robot, Act::Details),
                    item("Background…", IconName::Image, Act::Background),
                ],
                vec![item("Delete", IconName::Trash, Act::Remove).danger()],
            ]),
            Target::Project(id) => {
                // A project on an SSH host has no folder here to show.
                let remote = state
                    .settings
                    .projects
                    .iter()
                    .any(|p| &p.id == id && patchwork_platform::ssh::is_remote(&p.path));
                let mut tasks = vec![
                    item("Rename", IconName::PencilSimple, Act::Rename),
                    item(
                        "New task in a worktree",
                        IconName::Folders,
                        Act::WorktreeTask,
                    ),
                ];
                if !remote && !state.settings.worktrees.disabled {
                    let in_place = state.settings.edit_in_place.iter().any(|p| p == id);
                    tasks.push(if in_place {
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
                let mut settings = Vec::new();
                if state.settings.granular_chats {
                    settings.push(item("Granular settings…", IconName::Gauge, Act::Granular));
                }
                settings.push(item("Change folder…", IconName::Folder, Act::ChangeFolder));
                if !remote {
                    settings.push(item("Open folder", IconName::FolderOpen, Act::OpenFolder));
                }
                grouped([
                    tasks,
                    settings,
                    vec![item("Remove", IconName::Trash, Act::Remove).danger()],
                ])
            }
            Target::Folder(_) => grouped([
                vec![item("Rename", IconName::PencilSimple, Act::Rename)],
                vec![item("Remove", IconName::Trash, Act::Remove).danger()],
            ]),
        }
    }

    /// A chat's menu: its tabs and panes, how it is listed, what else it does, and archiving it.
    fn chat_items(&self, id: &str, cx: &gpui::App) -> Vec<Item> {
        let item = Item::new;
        let state = AppState::get(cx);
        let mut tabs = Vec::new();
        if self.has_tab(id) {
            tabs.push(item("Close tab", IconName::X, Act::CloseTab));
            let (others, right) = self.tabs_beside(id);
            if others {
                tabs.push(item(
                    "Close other tabs",
                    IconName::XCircle,
                    Act::CloseOthers,
                ));
            }
            if right {
                tabs.push(item(
                    "Close tabs to the right",
                    IconName::XCircle,
                    Act::CloseRight,
                ));
            }
        } else {
            tabs.push(item("Open in new tab", IconName::Plus, Act::OpenInNewTab));
        }
        if self.can_split(id, cx) {
            tabs.push(Item::submenu(
                "Split",
                IconName::SquareSplitHorizontal,
                vec![
                    item(
                        "Split right",
                        IconName::SquareSplitHorizontal,
                        Act::Split(Zone::Right),
                    ),
                    item(
                        "Split down",
                        IconName::SquareSplitVertical,
                        Act::Split(Zone::Bottom),
                    ),
                ],
            ));
        }
        let mut listed = vec![item("Rename", IconName::PencilSimple, Act::Rename)];
        if let Some(chat) = self.chat_by_id(id, cx) {
            listed.push(if chat.favorite {
                item(
                    "Remove from favorites",
                    IconName::Star,
                    Act::Favorite(false),
                )
            } else {
                item("Add to favorites", IconName::Star, Act::Favorite(true))
            });
            if chat.project_id.is_some() {
                listed.push(if chat.pinned {
                    item("Unpin", IconName::PushPinSlash, Act::Pin(false))
                } else {
                    item("Pin", IconName::PushPin, Act::Pin(true))
                });
            }
        }
        let mut more = Vec::new();
        if state.settings.granular_chats {
            more.push(item("Granular settings…", IconName::Gauge, Act::Granular));
        }
        more.extend(self.chat_app_items(id, cx));
        more.push(item("Export to…", IconName::Export, Act::Export));
        if state.settings.logging.chat_menu {
            more.push(item("Open logs", IconName::FileText, Act::OpenLogs));
        }
        let archive = item("Archive", IconName::Archive, Act::Remove).keys(DeleteChat);
        grouped([tabs, listed, more, vec![archive]])
    }

    /// The items that stop a chat app's conversation talking to the chat `id`.
    fn chat_app_items(&self, id: &str, cx: &gpui::App) -> Vec<Item> {
        let bound = AppState::get(cx)
            .chat_apps
            .get(id)
            .map_or(&[][..], |apps| apps.conversations.as_slice());
        bound
            .iter()
            // A conversation whose agent only works with this chat talks to another.
            .filter(|b| b.chat_id == id)
            .map(|b| {
                Item::new(
                    format!("Stop continuing on {}", b.platform.label()),
                    IconName::X,
                    Act::StopOn(b.platform, b.conversation.clone()),
                )
            })
            .collect()
    }

    /// The items for rows picked together: favorites and pins when they are all chats or tasks,
    /// and the one that archives or removes them all.
    fn many_items(&self, targets: &[Target], cx: &gpui::App) -> Vec<Item> {
        let chats: Vec<&Chat> = targets
            .iter()
            .filter_map(|t| match t {
                Target::Chat(id) => self.chat_by_id(id, cx),
                _ => None,
            })
            .collect();
        let archiving = chats.len() == targets.len();
        let item = |label: &str, icon, act| Item::new(label.to_string(), icon, act);
        let mut open = Vec::new();
        let project = self.tabs_project(cx);
        if archiving
            && (2..=4).contains(&chats.len())
            && chats.iter().all(|c| c.project_id == project)
        {
            open.push(item(
                "Open side by side",
                IconName::SquareSplitHorizontal,
                Act::SideBySide,
            ));
        }
        let mut listed = Vec::new();
        if archiving {
            // Some not yet a favorite, or not yet pinned: the item adds them all; otherwise it
            // takes them all off.
            let favorite = chats.iter().any(|t| !t.favorite);
            listed.push(if favorite {
                item("Add to favorites", IconName::Star, Act::Favorite(true))
            } else {
                item(
                    "Remove from favorites",
                    IconName::Star,
                    Act::Favorite(false),
                )
            });
            if chats.iter().all(|t| t.project_id.is_some()) {
                listed.push(if chats.iter().any(|t| !t.pinned) {
                    item("Pin", IconName::PushPin, Act::Pin(true))
                } else {
                    item("Unpin", IconName::PushPinSlash, Act::Pin(false))
                });
            }
        }
        let n = targets.len();
        let remove = if archiving {
            item(
                &format!("Archive {n} items"),
                IconName::Archive,
                Act::Remove,
            )
        } else {
            item(&format!("Remove {n} items"), IconName::Trash, Act::Remove).danger()
        };
        grouped([open, listed, vec![remove]])
    }

    /// A tab strip's menu: split the pane, show it alone, close it.
    fn pane_items(&self, pane: PaneId, cx: &gpui::App) -> Vec<Item> {
        let item = Item::new;
        let layout = self.layouts.get(&self.tabs_project(cx));
        let several = layout.is_some_and(|l| l.panes.len() > 1);
        let maximized = layout.is_some_and(|l| l.maximized.is_some());
        // The keys act on the focused pane, so only its menu shows them.
        let focused = layout.is_none_or(|l| l.focused == pane);
        let keyed = |item: Item, action: Box<dyn gpui::Action>| {
            if focused {
                Item {
                    keys: Some(std::rc::Rc::from(action)),
                    ..item
                }
            } else {
                item
            }
        };
        let split = vec![
            keyed(
                item(
                    "Split right",
                    IconName::SquareSplitHorizontal,
                    Act::SplitPane(Zone::Right),
                ),
                Box::new(SplitPaneRight),
            ),
            keyed(
                item(
                    "Split down",
                    IconName::SquareSplitVertical,
                    Act::SplitPane(Zone::Bottom),
                ),
                Box::new(SplitPaneDown),
            ),
        ];
        let mut view = Vec::new();
        if maximized {
            view.push(keyed(
                item("Show all panes", IconName::ArrowsInSimple, Act::Maximize),
                Box::new(MaximizePane),
            ));
        } else if several && layout.is_some_and(|l| l.focused == pane) {
            view.push(keyed(
                item("Maximize pane", IconName::ArrowsOutSimple, Act::Maximize),
                Box::new(MaximizePane),
            ));
        }
        let mut swap = Vec::new();
        let mut close = Vec::new();
        if several {
            view.push(if self.broadcasting {
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
                let sides: [(&str, Dir, Box<dyn gpui::Action>); 4] = [
                    ("Pane left", Dir::Left, Box::new(SwapPaneLeft)),
                    ("Pane right", Dir::Right, Box::new(SwapPaneRight)),
                    ("Pane above", Dir::Up, Box::new(SwapPaneUp)),
                    ("Pane below", Dir::Down, Box::new(SwapPaneDown)),
                ];
                let sides: Vec<Item> = sides
                    .into_iter()
                    .filter_map(|(label, dir, action)| {
                        let other = layout.neighbour(pane, dir)?;
                        let swap = item(label, IconName::ArrowsLeftRight, Act::SwapPane(other));
                        Some(keyed(swap, action))
                    })
                    .collect();
                if !sides.is_empty() {
                    swap.push(Item::submenu("Swap with", IconName::ArrowsLeftRight, sides));
                }
            }
            close.push(item("Close pane", IconName::X, Act::ClosePane));
        }
        grouped([split, view, swap, close])
    }

    /// The tab add button's menu: a new task, then each of the project's tasks with no tab.
    fn tab_items(&self, project: Option<&str>, cx: &gpui::App) -> Vec<Item> {
        let new = Item::new(
            if project.is_some() {
                "New task"
            } else {
                "New chat"
            },
            IconName::Plus,
            Act::NewTaskTab,
        );
        let tasks = self
            .untabbed_tasks(project, cx)
            .into_iter()
            .map(|c| {
                let title = patchwork_design_text::display_title(&c.title);
                let label: SharedString = if title.trim().is_empty() {
                    "New chat".into()
                } else {
                    title.into()
                };
                Item::new(label, IconName::ChatCircle, Act::OpenTab(c.id.clone()))
            })
            .collect();
        grouped([vec![new], tasks])
    }

    /// The menu rows for `items`, each acting on `target` in this window.
    fn menu_items(
        items: Vec<Item>,
        target: &Target,
        workspace: &gpui::WeakEntity<Self>,
    ) -> Vec<MenuItem> {
        items
            .into_iter()
            .map(|item| {
                let row = if let Some(items) = item.submenu {
                    MenuItem::submenu(item.label, Self::menu_items(items, target, workspace))
                } else {
                    let (workspace, target, act) = (workspace.clone(), target.clone(), item.act);
                    MenuItem::new(item.label, move |window, cx| {
                        let _ = workspace.update(cx, |this, cx| {
                            this.act(target.clone(), act.clone(), window, cx);
                        });
                    })
                };
                MenuItem {
                    icon: Some(item.icon),
                    danger: item.danger,
                    separated: item.separated,
                    keys: item.keys,
                    ..row
                }
            })
            .collect()
    }

    /// Opens the menu for `target` where the pointer pressed.
    pub fn open_context_menu(
        &mut self,
        target: Target,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items = Self::menu_items(self.items(&target, cx), &target, &cx.weak_entity());
        let menu = Menu::open("Actions", items, window, cx);
        // The press otherwise goes on to focus the window under the row, and the menu, without
        // the focus, hears none of its keys.
        window.prevent_default();
        let closed = cx.subscribe_in(&menu, window, |this, _, _: &Dismissed, window, cx| {
            this.close_context_menu(window, cx);
        });
        self.context_menu = Some(ContextMenu {
            at,
            target,
            menu,
            _closed: closed,
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
        let picked = self.sidebar.read(cx).picked();
        let clicked = self.sidebar.read(cx).clicked();
        let target = if let Some(menu) = &self.context_menu {
            menu.target.clone()
        } else if !picked.is_empty() {
            Target::Many(picked.into_iter().map(Target::from).collect())
        } else if let Some(patchwork_desktop_sidebar::Row::Chat(id)) = clicked
            && self
                .chat
                .as_ref()
                .is_some_and(|t| t.id == id && t.parent.is_none())
        {
            Target::Chat(id)
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
            (_, Act::Rename) => {
                if let Some(row) = target.row() {
                    self.sidebar
                        .update(cx, |s, cx| s.start_rename(row, window, cx));
                }
                return;
            }
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
                let change = move |core: &Arc<patchwork_core::Core>| match act {
                    Act::Favorite(on) => core.set_chat_favorite(&id, on),
                    Act::Pin(on) => core.set_chat_pinned(&id, on),
                    _ => core.chats().archive(&id, false),
                };
                return self.change("change the task list", change, cx);
            }
            (Target::Chat(id), Act::Remove)
                if self
                    .chat_by_id(id, cx)
                    .is_some_and(|t| t.archived_at.is_some()) =>
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
                let change = move |core: &Arc<patchwork_core::Core>| {
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
                    move |core: &Arc<patchwork_core::Core>| core.set_edits_in_place(&id, on);
                return self.change("change where the project's tasks work", change, cx);
            }
            (Target::Project(id), Act::ChangeFolder) => {
                return self.change_project_folder(id.clone(), cx);
            }
            (Target::Project(id), Act::OpenFolder) => {
                let Some(project) = AppState::get(cx)
                    .settings
                    .projects
                    .iter()
                    .find(|p| &p.id == id)
                else {
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
                let change = move |core: &Arc<patchwork_core::Core>| {
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
