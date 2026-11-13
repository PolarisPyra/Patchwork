//! The sidebar's tasks view: the projects and their chats.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use super::menu::{self, Target};
use gpui::{
    ClickEvent, FontWeight, MouseButton, MouseDownEvent, Role, SharedString, div, prelude::*, px,
};
use patchwork_server::system::remote;
use patchwork_server::threads::Thread;
use crate::ui::theme::theme;
use crate::ui::{button, icon, icon_button, scrollbar};
use crate::workspace::Workspace;
use super::render::{FS_SM, FS_XS, empty_row, section_label, twistie};
use super::tree::{TREE_GUTTER, indent_guide, reveal_button, thread_row, tree_row};

pub(super) fn tasks_view(w: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let t = *theme(cx);
    let live: Vec<Arc<Thread>> = w
        .threads
        .iter()
        .filter(|th| th.archived_at.is_none() && th.parent.is_none())
        .cloned()
        .collect();
    let selected = w.thread.as_ref().map(|th| th.id.clone());
    // The rows in the order they are drawn, for Shift+click to pick a run of them.
    let mut rows: Vec<Target> = Vec::new();
    let favorites: Vec<&Thread> = live
        .iter()
        .map(Arc::as_ref)
        .filter(|th| th.favorite)
        .collect();
    let chats: Vec<&Thread> = live
        .iter()
        .map(Arc::as_ref)
        .filter(|th| th.project_id.is_none() && !th.favorite)
        .collect();

    let mut tree = div()
        .id("tasks")
        .role(Role::Tree)
        .aria_label("Tasks")
        .group("tree")
        .track_scroll(&w.tree_scroll)
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(1.))
        .size_full()
        .p(px(8.))
        // The web tree's `overflow-y: scroll` keeps a 10px scrollbar gutter, so rows end clear of it.
        .pr(px(8. + TREE_GUTTER));

    if !favorites.is_empty() {
        tree = tree.child(section_label("Favorites", true, None, &t));
        for th in &favorites {
            rows.push(Target::Thread(th.id.clone()));
            tree = tree.child(thread_row(w, th, 0, selected.as_deref(), cx));
        }
    }
    let new_chat = icon_button("new-chat", "New chat", "plus", 18., cx)
        .on_click(cx.listener(|this, _, window, cx| this.new_thread_in(None, None, window, cx)))
        .into_any_element();
    tree = tree.child(section_label(
        "Chats",
        favorites.is_empty(),
        Some(new_chat),
        &t,
    ));
    if chats.is_empty() {
        tree = tree.child(empty_row("No chats yet", &t));
    }
    for th in &chats {
        rows.push(Target::Thread(th.id.clone()));
        tree = tree.child(thread_row(w, th, 0, selected.as_deref(), cx));
    }

    let (muted, fg) = (t.text_muted, t.text);
    let projects_action = div()
        .flex()
        .items_center()
        .gap(px(4.))
        .child(
            button("collapse-all", "Collapse all")
                .gap(px(4.))
                .text_color(muted)
                .hover(move |s| s.text_color(fg))
                .child(icon("arrows-in-line-vertical", 15.))
                .child("Collapse all")
                .on_click(cx.listener(|this, _, _, cx| this.collapse_all(cx))),
        )
        .child(
            icon_button("open-folder", "Open folder", "folder-plus", 18., cx)
                .on_click(cx.listener(|this, _, window, cx| this.open_folder(window, cx))),
        )
        .into_any_element();
    tree = tree.child(section_label("Projects", false, Some(projects_action), &t));
    if w.settings.projects.is_empty() {
        tree = tree.child(empty_row("No projects yet", &t));
    }

    let mut projects = div().mx(px(-8.)).flex_none();
    for p in &w.settings.projects {
        let open = !w.collapsed.contains(&p.id);
        let tasks: Vec<&Thread> = live
            .iter()
            .map(Arc::as_ref)
            .filter(|th| th.project_id.as_ref() == Some(&p.id) && !th.favorite)
            .collect();
        let pinned: Vec<&&Thread> = tasks.iter().filter(|th| th.pinned).collect();
        let unpinned: Vec<&&Thread> = tasks.iter().filter(|th| !th.pinned).collect();
        let folders: Vec<_> = w
            .folders
            .iter()
            .filter(|f| f.project_id == p.id)
            .cloned()
            .collect();
        let id = p.id.clone();
        let target = Target::Project(p.id.clone());
        rows.push(target.clone());
        let picked = w.is_picked(&target);
        let pick = target.clone();
        let name = w.rename_field(&target, cx).unwrap_or_else(|| {
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(p.name.clone())
                .into_any_element()
        });
        let sorted = folders.iter().any(|f| menu::is_agent_folder(&f.name));
        let (sort_id, folder_id, task_id) = (p.id.clone(), p.id.clone(), p.id.clone());
        let key = SharedString::from(format!("project:{}", p.id));
        let shown = w.hovered_row.as_ref() == Some(&key);
        // A project on an SSH host names the host after its own name.
        let host = remote::split(&p.path).map(|l| l.host.to_string());
        let label = match &host {
            Some(host) => format!("{} on {host}", p.name),
            None => p.name.clone(),
        };
        let row = tree_row(key, label, 1, open, picked, &t, cx)
            .pl(px(4.))
            .child(twistie(open))
            .child(name)
            .when_some(host.filter(|_| !shown), |row, host| {
                row.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(FS_XS))
                        .text_color(t.text_subtle)
                        .child(icon("hard-drives", 12.))
                        .child(host),
                )
            })
            .when(shown && sorted, |row| {
                let undo_id = p.id.clone();
                row.child(
                    reveal_button(
                        &format!("undo-sort:{}", p.id),
                        format!("Undo sorting tasks in {} by provider", p.name),
                        "arrow-counter-clockwise",
                        &t,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.undo_sort_by_agent(&undo_id, cx);
                    })),
                )
            })
            .when(shown, |row| {
                row.child(
                    reveal_button(
                        &format!("sort:{}", p.id),
                        format!("Sort tasks in {} into folders by provider", p.name),
                        "magic-wand",
                        &t,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.sort_by_agent(&sort_id, cx);
                    })),
                )
                .child(
                    reveal_button(
                        &format!("folder:{}", p.id),
                        format!("New folder in {}", p.name),
                        "folder-plus",
                        &t,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.new_folder(&folder_id, window, cx);
                    })),
                )
                .child(
                    reveal_button(
                        &format!("task:{}", p.id),
                        format!("New task in {}", p.name),
                        "plus",
                        &t,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.new_thread_in(Some(task_id.clone()), None, window, cx);
                    })),
                )
            })
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    let target = this.menu_target(target.clone());
                    this.open_context_menu(target, e, window, cx);
                }),
            )
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                if !this.pick_row(&pick, e.modifiers(), cx) {
                    this.toggle_collapsed(&id, cx);
                }
            }));

        let mut group = div().flex_none().child(row);
        if open {
            let mut children = div()
                .relative()
                .flex()
                .flex_col()
                .py(px(1.))
                .child(indent_guide(16.));
            if tasks.is_empty() && folders.is_empty() {
                children = children.child(
                    div()
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .pl(px(30.))
                        .text_size(px(FS_SM))
                        .text_color(t.text_muted)
                        .child("No tasks yet"),
                );
            }
            for th in &pinned {
                rows.push(Target::Thread(th.id.clone()));
                children = children.child(thread_row(w, th, 1, selected.as_deref(), cx));
            }
            for f in &folders {
                let key = format!("folder:{}", f.id);
                let open = !w.collapsed.contains(&key);
                let toggle_key = key.clone();
                let target = Target::Folder(f.id.clone());
                rows.push(target.clone());
                let picked = w.is_picked(&target);
                let pick = target.clone();
                let name = w.rename_field(&target, cx).unwrap_or_else(|| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(FS_SM))
                        .child(f.name.clone())
                        .into_any_element()
                });
                let (project_id, folder_id) = (p.id.clone(), f.id.clone());
                let key = SharedString::from(key);
                let shown = w.hovered_row.as_ref() == Some(&key);
                let folder = tree_row(key, f.name.clone(), 2, open, picked, &t, cx)
                    .pl(px(12.))
                    .child(twistie(open))
                    .child(name)
                    .when(shown, |row| {
                        row.child(
                            reveal_button(
                                &format!("folder-task:{}", f.id),
                                format!("New task in {}", f.name),
                                "plus",
                                &t,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    let (project, folder) = (project_id.clone(), folder_id.clone());
                                    this.new_thread_in(Some(project), Some(&folder), window, cx);
                                },
                            )),
                        )
                    })
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                            let target = this.menu_target(target.clone());
                            this.open_context_menu(target, e, window, cx);
                        }),
                    )
                    .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                        if !this.pick_row(&pick, e.modifiers(), cx) {
                            this.toggle_collapsed(&toggle_key, cx);
                        }
                    }));
                children = children.child(folder);
                if open {
                    let mut inside = div().relative().py(px(1.)).child(indent_guide(24.));
                    for th in unpinned
                        .iter()
                        .filter(|th| th.folder_id.as_ref() == Some(&f.id))
                    {
                        rows.push(Target::Thread(th.id.clone()));
                        inside = inside.child(thread_row(w, th, 2, selected.as_deref(), cx));
                    }
                    children = children.child(inside);
                }
            }
            for th in unpinned.iter().filter(|th| th.folder_id.is_none()) {
                rows.push(Target::Thread(th.id.clone()));
                children = children.child(thread_row(w, th, 1, selected.as_deref(), cx));
            }
            group = group.child(children);
        }
        projects = projects.child(group);
    }
    tree = tree.child(projects);
    w.picking.tree = rows;

    div()
        .relative()
        .flex_1()
        .min_h_0()
        .child(tree)
        .children(scrollbar(&w.tree_scroll, TREE_GUTTER, 2., t.border_strong))
}
