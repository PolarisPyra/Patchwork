//! The task tree's right-click menu (`components/ContextMenu.tsx` with `TasksView`'s items), the
//! name field that renames a row in place (`InlineName`), and the project buttons that make
//! folders and sort tasks into them by provider.

use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, KeyDownEvent, MouseDownEvent, Pixels,
    Point, Role, SharedString, Window, anchored, deferred, div, prelude::*, px,
};
use patchwork_server::threads::Thread;

use crate::chat::text_area::{Changed, Submitted, TextArea};
use crate::ui::theme::{RADIUS, RADIUS_SM, theme};
use crate::ui::{AGENT_KINDS, agent_label, icon, popup_shadow};
use crate::workspace::Workspace;

/// A row the menu or the name field acts on.
#[derive(Clone, PartialEq, Eq)]
pub enum Target {
    Thread(String),
    Project(String),
    Folder(String),
    /// An archived view header: the archived chats or tasks under it.
    Archived(Vec<String>),
    /// Rows picked together with Ctrl or Shift.
    Many(Vec<Target>),
}

/// The open menu: where it was opened, for which row, and the row the keyboard is on.
pub struct ContextMenu {
    at: Point<Pixels>,
    target: Target,
    focus: FocusHandle,
    highlight: Option<usize>,
}

/// The name field in place of a row's name.
pub struct Renaming {
    pub target: Target,
    pub field: Entity<TextArea>,
    /// What it was called, so an unchanged name saves nothing.
    current: String,
}

#[derive(Clone, Copy)]
enum Act {
    Rename,
    Favorite(bool),
    Pin(bool),
    Background,
    Export,
    OpenLogs,
    WorktreeTask,
    ChangeFolder,
    OpenFolder,
    Remove,
    Restore,
}

struct Item {
    label: SharedString,
    icon: &'static str,
    danger: bool,
    act: Act,
}

/// Where each task of a project was before it was sorted by provider, in order.
pub type SortUndo = Vec<(String, Option<String>)>;

impl Workspace {
    pub(super) fn thread_by_id(&self, id: &str) -> Option<&Thread> {
        self.threads.iter().find(|t| t.id == id)
    }

    fn items(&self, target: &Target) -> Vec<Item> {
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
        self.context_menu = Some(ContextMenu {
            at: event.position,
            target,
            focus,
            highlight: None,
        });
        cx.notify();
    }

    fn close_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn act(&mut self, target: Target, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        self.close_context_menu(window, cx);
        let done = match (&target, act) {
            (_, Act::Rename) => return self.start_rename(target, window, cx),
            (Target::Thread(id), Act::Favorite(on)) => self.server.set_thread_favorite(id, on),
            (Target::Thread(id), Act::Pin(on)) => self.server.set_thread_pinned(id, on),
            (Target::Thread(id), Act::Restore) => self.server.set_thread_archived(id, false),
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
            (Target::Thread(id), Act::Export) => return self.export_thread(id, cx),
            (Target::Thread(id), Act::OpenLogs) => {
                let (server, id) = (self.server.clone(), id.clone());
                cx.background_spawn(async move {
                    let shown = server.thread_report(&id).and_then(|path| {
                        patchwork_server::system::launch::reveal(&path, None)
                            .map_err(|e| e.to_string())
                    });
                    if let Err(e) = shown {
                        log::warn!("could not show the logs of the chat: {e}");
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
                let path = std::path::PathBuf::from(&project.path);
                cx.background_spawn(async move {
                    if let Err(e) = patchwork_server::system::launch::reveal(&path, None) {
                        log::warn!("could not show {}: {e}", path.display());
                    }
                })
                .detach();
                Ok(())
            }
            (Target::Project(_) | Target::Folder(_), Act::Remove) => {
                return self.remove_targets(vec![target], window, cx);
            }
            (Target::Many(targets), Act::Remove) => {
                return self.remove_targets(targets.clone(), window, cx);
            }
            (Target::Many(targets), Act::Favorite(_) | Act::Pin(_)) => targets
                .iter()
                .filter_map(|t| match t {
                    Target::Thread(id) => Some(id),
                    _ => None,
                })
                .try_for_each(|id| match act {
                    Act::Favorite(on) => self.server.set_thread_favorite(id, on),
                    _ => self
                        .server
                        .set_thread_pinned(id, matches!(act, Act::Pin(true))),
                }),
            _ => Ok(()),
        };
        if let Err(e) = done {
            log::warn!("could not change the task list: {e}");
        }
        self.reload(cx);
    }

    /// Shows the name field in place of the row's name, holding its name.
    pub fn start_rename(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let current = match &target {
            Target::Thread(id) => self.thread_by_id(id).map(|t| t.title.clone()),
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
            Target::Archived(_) | Target::Many(_) => None,
        };
        let Some(current) = current else {
            return;
        };
        let t = *theme(cx);
        let field = cx.new(|cx| {
            let mut field = TextArea::new("", "Name", cx);
            field.single_line = true;
            field.text_color = t.text;
            field.placeholder_color = t.text_subtle;
            field.selection_color = t.accent.opacity(0.3);
            field.set_text(&current, cx);
            field
        });
        cx.subscribe_in(&field, window, |this, _, _: &Submitted, window, cx| {
            this.finish_rename(true, window, cx)
        })
        .detach();
        cx.subscribe(&field, |_, _, _: &Changed, cx| cx.notify())
            .detach();
        let focus = field.read(cx).focus_handle(cx);
        cx.on_blur(&focus, window, |this, window, cx| {
            this.finish_rename(true, window, cx)
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
        if save && !name.is_empty() && name != renaming.current {
            let done = match &renaming.target {
                Target::Thread(id) => self.server.rename_thread(id, &name),
                Target::Project(id) => self.server.rename_project(id, &name).map(|()| {
                    self.settings = self.server.store.settings();
                }),
                Target::Folder(id) => self.server.rename_folder(id, &name),
                Target::Archived(_) | Target::Many(_) => Ok(()),
            };
            if let Err(e) = done {
                log::warn!("could not rename it: {e}");
            }
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
                .rounded(px(RADIUS_SM))
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

    /// Makes a folder called "New folder" in the project, opens the project, and names it.
    pub fn new_folder(&mut self, project_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match self.server.create_folder(project_id, "New folder") {
            Ok(folder) => {
                if self.collapsed.remove(project_id) {
                    self.save_collapsed();
                }
                self.reload(cx);
                self.start_rename(Target::Folder(folder.id), window, cx);
            }
            Err(e) => log::warn!("could not make a folder: {e}"),
        }
    }

    /// Moves the project's tasks into a folder per provider, made as needed, and remembers where
    /// they were so it can be undone.
    pub fn sort_by_agent(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let tasks: Vec<Thread> = self
            .threads
            .iter()
            .filter(|t| {
                t.project_id.as_deref() == Some(project_id)
                    && t.archived_at.is_none()
                    && t.parent.is_none()
            })
            .cloned()
            .collect();
        for agent in AGENT_KINDS {
            let ids: Vec<String> = tasks
                .iter()
                .filter(|t| t.agent == agent)
                .map(|t| t.id.clone())
                .collect();
            if ids.is_empty() {
                continue;
            }
            let name = agent_label(agent);
            let folder = match self
                .folders
                .iter()
                .find(|f| f.project_id == project_id && f.name == name)
            {
                Some(folder) => Ok(folder.id.clone()),
                None => self.server.create_folder(project_id, name).map(|f| {
                    let id = f.id.clone();
                    self.folders.push(f);
                    id
                }),
            };
            if let Err(e) = folder.and_then(|f| self.server.reorder_threads(&ids, Some(&f))) {
                log::warn!("could not sort the tasks: {e}");
            }
        }
        // A task sorted before keeps the place it had then, not the provider folder it is in now.
        let kept = self.sort_undo.remove(project_id).unwrap_or_default();
        let before: SortUndo = kept
            .iter()
            .cloned()
            .chain(
                tasks
                    .iter()
                    .filter(|t| !kept.iter().any(|(id, _)| id == &t.id))
                    .map(|t| (t.id.clone(), t.folder_id.clone())),
            )
            .collect();
        self.sort_undo.insert(project_id.to_string(), before);
        self.reload(cx);
    }

    /// Puts the tasks in provider folders back where they were before sorting, or at the top of
    /// the project.
    pub fn undo_sort_by_agent(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let agent_folders: Vec<String> = self
            .folders
            .iter()
            .filter(|f| f.project_id == project_id && is_agent_folder(&f.name))
            .map(|f| f.id.clone())
            .collect();
        let before = self.sort_undo.remove(project_id).unwrap_or_default();
        let destination = |t: &Thread| -> Option<String> {
            let folder = t.folder_id.clone()?;
            if !agent_folders.contains(&folder) {
                return Some(folder);
            }
            before
                .iter()
                .find(|(id, _)| id == &t.id)
                .and_then(|(_, prior)| prior.clone())
                .filter(|prior| {
                    !agent_folders.contains(prior) && self.folders.iter().any(|f| &f.id == prior)
                })
        };
        let mut lists: Vec<(Option<String>, Vec<&Thread>)> = Vec::new();
        for t in &self.threads {
            if t.project_id.as_deref() != Some(project_id)
                || t.archived_at.is_some()
                || t.parent.is_some()
            {
                continue;
            }
            let to = destination(t);
            match lists.iter_mut().find(|(f, _)| *f == to) {
                Some((_, list)) => list.push(t),
                None => lists.push((to, vec![t])),
            }
        }
        let rank = |id: &str| {
            before
                .iter()
                .position(|(b, _)| b == id)
                .unwrap_or(before.len())
        };
        for (folder, mut list) in lists {
            if list.iter().all(|t| t.folder_id == folder) {
                continue;
            }
            list.sort_by_key(|t| rank(&t.id));
            let ids: Vec<String> = list.iter().map(|t| t.id.clone()).collect();
            if let Err(e) = self.server.reorder_threads(&ids, folder.as_deref()) {
                log::warn!("could not undo the sort: {e}");
            }
        }
        for id in agent_folders {
            if let Err(e) = self.server.remove_folder(&id) {
                log::warn!("could not remove a provider folder: {e}");
            }
        }
        self.reload(cx);
    }

    /// The open menu, drawn over everything at the pointer.
    pub fn context_menu_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.context_menu.as_ref()?;
        let t = *theme(cx);
        let items = self.items(&menu.target);
        let count = items.len();
        let rows =
            items.into_iter().enumerate().map(|(i, item)| {
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
                        this.act(target.clone(), act, window, cx)
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
                        menu.highlight = Some(at.map_or(count - 1, |i| (i + count - 1) % count))
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

/// A folder sorting by provider made, and undoing it removes.
pub fn is_agent_folder(name: &str) -> bool {
    AGENT_KINDS.iter().any(|&kind| agent_label(kind) == name)
}

impl Workspace {
    /// Makes a worktree of the project, as a project of its own, and starts a task in it.
    pub(crate) fn new_task_in_worktree(
        &mut self,
        project_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let server = self.server.clone();
        let made = cx.background_spawn(async move { server.create_worktree(&project_id) });
        cx.spawn_in(window, async move |this, cx| {
            let made = made.await;
            let _ = this.update_in(cx, |this, window, cx| match made {
                Ok(project) => {
                    this.settings = this.server.store.settings();
                    this.new_thread_in(Some(project.id), None, window, cx);
                }
                Err(e) => {
                    this.error = Some(e);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Asks for the folder a moved project is in now, and points the project there.
    fn change_project_folder(&mut self, project_id: String, cx: &mut Context<Self>) {
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
                    Err(e) => this.error = Some(e),
                }
                this.reload(cx);
            });
        })
        .detach();
    }
}
