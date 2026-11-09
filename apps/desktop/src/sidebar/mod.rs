//! The sidebar (`features/sidebar`): the search button, the task tree, and the usage button, with
//! the edge that resizes it.

mod archived;
pub mod changes;
mod export;
mod graph;
pub mod menu;
pub mod pick;
mod remove;
pub mod usage;

use archived::archived_view;
use menu::Target;

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, IntoElement, MouseButton,
    MouseDownEvent, Role, SharedString, Stateful, Window, div, prelude::*, px,
};
use patchwork_server::threads::{ApprovalStatus, Thread};

use crate::theme::{RADIUS_SM, Theme, theme};
use crate::ui::{
    agent_label, agent_logo, button, display_title, format_relative, icon, icon_button, scrollbar,
    spinner,
};
use crate::workspace::{SIDEBAR_MAX, SIDEBAR_MIN, View, Workspace, is_busy};

/// The sidebar's own type scale: its small text is a step larger than the chat's.
const FS_XS: f32 = 12.;
const FS_SM: f32 = 13.;

pub fn render(
    w: &mut Workspace,
    view: View,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    let t = *theme(cx);
    let width = w.sidebar_width;
    let (border_strong, fg) = (t.border_strong, t.text);
    let body = match view {
        View::Tasks => tasks_view(w, cx).into_any_element(),
        View::Archived => archived_view(w, cx).into_any_element(),
        View::Changes => div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(w.changes_view.clone())
            .into_any_element(),
    };
    div()
        .id("sidebar")
        .role(Role::Complementary)
        .aria_label("Sidebar")
        .relative()
        .flex()
        .flex_col()
        .flex_none()
        .min_h_0()
        .w(px(width))
        .border_r_1()
        .border_color(t.border)
        .bg(t.bg_panel)
        .text_size(px(13.))
        .child(
            div()
                .id("sidebar-resize")
                .role(Role::Splitter)
                .aria_orientation(gpui::Orientation::Vertical)
                .aria_label("Resize sidebar")
                .aria_numeric_value(width as f64)
                .aria_min_numeric_value(SIDEBAR_MIN as f64)
                .aria_max_numeric_value(SIDEBAR_MAX as f64)
                .tab_index(0)
                .absolute()
                .top_0()
                .bottom_0()
                .right(px(-3.))
                .w(px(6.))
                .cursor_col_resize()
                .hover(move |s| s.bg(t.accent.opacity(0.4)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, e: &MouseDownEvent, _, cx| {
                        this.start_resize(f32::from(e.position.x));
                        cx.stop_propagation();
                    }),
                )
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| {
                    let step = match e.keystroke.key.as_str() {
                        "left" => -16.,
                        "right" => 16.,
                        _ => return,
                    };
                    this.sidebar_width =
                        (this.sidebar_width + step).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                    cx.notify();
                }))
                .on_click(cx.listener(|this, e: &gpui::ClickEvent, _, cx| {
                    if e.click_count() == 2 {
                        this.sidebar_width = crate::workspace::SIDEBAR_WIDTH;
                        cx.notify();
                    }
                })),
        )
        .when(view != View::Changes, |s| {
            s.child(
                button("search", "Search…")
                    .mx(px(8.))
                    .mt(px(12.))
                    .mb(px(2.))
                    .h(px(32.))
                    .flex_none()
                    .gap(px(6.))
                    .rounded(px(RADIUS_SM))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.bg_input)
                    .px(px(8.))
                    .text_size(px(FS_SM))
                    .text_color(t.text_muted)
                    .hover(move |s| s.border_color(border_strong).text_color(fg))
                    .child(icon("magnifying-glass", 18.))
                    .child("Search…")
                    .child(
                        div()
                            .ml_auto()
                            .min_w(px(20.))
                            .h(px(20.))
                            .px(px(5.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .bg(t.bg_selected_muted)
                            .text_color(t.text_muted)
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Ctrl K"),
                    )
                    .on_click(cx.listener(|w, _, window, cx| w.toggle_palette(window, cx))),
            )
        })
        .child(body)
        .child(usage::footer(w, &t, cx))
        .children(w.context_menu_view(cx))
}

fn empty_row(text: &'static str, t: &Theme) -> Div {
    div()
        .h(px(28.))
        .flex_none()
        .flex()
        .items_center()
        .px(px(6.))
        .text_size(px(FS_SM))
        .text_color(t.text_muted)
        .child(text)
}

/// `SectionLabel`: a small uppercase heading with its action on the right.
fn section_label(text: &str, first: bool, action: Option<AnyElement>, t: &Theme) -> Div {
    div()
        .flex_none()
        .mr(px(-2.))
        .flex()
        .items_center()
        .justify_between()
        .pt(px(if first { 4. } else { 10. }))
        .pl(px(6.))
        .text_size(px(FS_XS))
        .text_color(t.text_muted)
        .child(div().child(text.to_uppercase()))
        .children(action)
}

/// The chevron of a project or folder, pointing right while it is closed.
fn twistie(open: bool) -> impl IntoElement {
    div()
        .w(px(16.))
        .flex_none()
        .flex()
        .justify_center()
        .ml(px(3.))
        .mr(px(3.))
        .child(icon(if open { "twistie" } else { "twistie-closed" }, 16.))
}

fn tasks_view(w: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let t = *theme(cx);
    let live: Vec<Thread> = w
        .threads
        .iter()
        .filter(|th| th.archived_at.is_none() && th.parent.is_none())
        .cloned()
        .collect();
    let selected = w.thread.as_ref().map(|th| th.id.clone());
    // The rows in the order they are drawn, for Shift+click to pick a run of them.
    let mut rows: Vec<Target> = Vec::new();
    let favorites: Vec<&Thread> = live.iter().filter(|th| th.favorite).collect();
    let chats: Vec<&Thread> = live
        .iter()
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
    for p in w.settings.projects.clone() {
        let open = !w.collapsed.contains(&p.id);
        let tasks: Vec<&Thread> = live
            .iter()
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
        let row = tree_row(key, p.name.clone(), 1, open, picked, &t, cx)
            .pl(px(4.))
            .child(twistie(open))
            .child(name)
            .when(shown && sorted, |row| {
                let undo_id = p.id.clone();
                row.child(
                    reveal_button(
                        format!("undo-sort:{}", p.id),
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
                        format!("sort:{}", p.id),
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
                        format!("folder:{}", p.id),
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
                        format!("task:{}", p.id),
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
                    this.open_context_menu(target, e, window, cx)
                }),
            )
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                if !this.pick_row(&pick, &e.modifiers(), cx) {
                    this.toggle_collapsed(&id, cx)
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
                                format!("folder-task:{}", f.id),
                                format!("New task in {}", f.name),
                                "plus",
                                &t,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    let (project, folder) = (project_id.clone(), folder_id.clone());
                                    this.new_thread_in(Some(project), Some(folder), window, cx);
                                },
                            )),
                        )
                    })
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                            let target = this.menu_target(target.clone());
                            this.open_context_menu(target, e, window, cx)
                        }),
                    )
                    .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                        if !this.pick_row(&pick, &e.modifiers(), cx) {
                            this.toggle_collapsed(&toggle_key, cx)
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

/// The scrollbar's gutter at the tree's right edge, which its thumb sits in.
const TREE_GUTTER: f32 = 10.;

/// The line from a project's or folder's chevron down through its rows, shown while the tree is
/// hovered.
fn indent_guide(left: f32) -> Div {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(left))
        .w(px(1.))
        .invisible()
        .group_hover("tree", |s| s.visible())
        .bg(gpui::rgba(0x58585866))
}

/// A project or folder row, which notes when the pointer is over it so it can show its buttons.
fn tree_row(
    id: SharedString,
    name: String,
    level: usize,
    open: bool,
    picked: bool,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let (fill, hover) = if picked {
        (t.bg_accent_soft, t.tree_selected)
    } else {
        (gpui::transparent_black(), t.tree_hover)
    };
    let row = id.clone();
    div()
        .id(id)
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                this.hovered_row = Some(row.clone());
            } else if this.hovered_row.as_ref() == Some(&row) {
                this.hovered_row = None;
            }
            cx.notify();
        }))
        .role(Role::TreeItem)
        .aria_level(level)
        .aria_label(name)
        .aria_expanded(open)
        .aria_selected(picked)
        .tab_index(0)
        .mx(px(4.))
        .mb(px(2.))
        .h(px(28.))
        .flex_none()
        .flex()
        .items_center()
        .cursor_pointer()
        .rounded(px(RADIUS_SM))
        .pr(px(6.))
        .text_color(t.text)
        .bg(fill)
        .hover(move |s| s.bg(hover))
}

/// A 20px icon button a row shows while hovered.
fn reveal_button(id: String, label: String, name: &str, t: &Theme) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(SharedString::from(format!("reveal:{id}")), label)
        .size(px(20.))
        .flex_none()
        .justify_center()
        .rounded(px(RADIUS_SM))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon(name, 17.))
}

/// `ThreadRow`: a chat or task with its time, or what it is waiting on, and its agent's mark.
fn thread_row(
    w: &Workspace,
    th: &Thread,
    depth: usize,
    selected: Option<&str>,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let t = *theme(cx);
    let target = Target::Thread(th.id.clone());
    let picked = selected == Some(th.id.as_str()) || w.is_picked(&target);
    let last = th.messages.last();
    let needs_approval = last.is_some_and(|m| {
        m.approvals
            .iter()
            .any(|a| a.status == ApprovalStatus::Pending)
    });
    let busy = is_busy(th);
    let title = display_title(&th.title);
    let (ml, mr) = match depth {
        0 => (-2., -4.),
        1 => (30., 4.),
        _ => (38., 4.),
    };
    let group = SharedString::from(format!("row:{}", th.id));
    let id = th.id.clone();
    let archive_id = th.id.clone();
    let (fill, hover) = if picked {
        (t.bg_accent_soft, t.tree_selected)
    } else {
        (gpui::transparent_black(), t.tree_hover)
    };
    let mark: Option<AnyElement> = if th.favorite {
        Some(
            icon("star-fill", 13.)
                .text_color(t.text_muted)
                .into_any_element(),
        )
    } else if th.pinned {
        Some(
            icon("push-pin-fill", 13.)
                .text_color(t.text_muted)
                .into_any_element(),
        )
    } else {
        None
    };
    let state: AnyElement = if needs_approval {
        icon("shield-warning", 15.)
            .text_color(t.warning)
            .into_any_element()
    } else if busy {
        spinner(12., t.accent_text).into_any_element()
    } else {
        let at = last.map_or(th.created_at, |m| m.created_at);
        let color: Hsla = if picked { t.text_muted } else { t.text_subtle };
        div()
            .text_size(px(FS_XS))
            .text_color(color)
            .child(format_relative(at))
            .into_any_element()
    };
    let pick = target.clone();
    let name = w.rename_field(&target, cx).unwrap_or_else(|| {
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .child(title.clone())
            .into_any_element()
    });
    let (btn_hover, fg) = (t.border_strong, t.text);
    div()
        .id(SharedString::from(format!("thread:{}", th.id)))
        .group(group.clone())
        .role(Role::TreeItem)
        .aria_level(depth + 1)
        .aria_selected(picked)
        .aria_label(title.clone())
        .tab_index(0)
        .h(px(28.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .ml(px(ml))
        .mr(px(mr))
        .mb(px(2.))
        .pl(px(8.))
        .pr(px(8.))
        .rounded(px(RADIUS_SM))
        .cursor_pointer()
        .text_color(t.text)
        .bg(fill)
        .hover(move |s| s.bg(hover))
        .children(mark)
        .child(name)
        .child(
            div()
                .w(px(32.))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .group_hover(group.clone(), |s| s.invisible())
                        .child(state),
                )
                .child(
                    button(
                        SharedString::from(format!("archive:{}", th.id)),
                        format!("Archive {title}"),
                    )
                    .absolute()
                    .size(px(20.))
                    .justify_center()
                    .rounded(px(RADIUS_SM))
                    .text_color(t.text_muted)
                    .hover(move |s| s.bg(btn_hover).text_color(fg))
                    .invisible()
                    .group_hover(group, |s| s.visible())
                    .child(icon("archive", 15.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.archive(&archive_id, cx);
                    })),
                ),
        )
        .child(
            div()
                .id("agent")
                .role(Role::Image)
                .size(px(16.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .aria_label(agent_label(th.agent))
                .child(agent_logo(th.agent, th.model.as_deref(), 14.)),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                let target = this.menu_target(target.clone());
                this.open_context_menu(target, e, window, cx)
            }),
        )
        .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
            if !this.pick_row(&pick, &e.modifiers(), cx) {
                this.open_thread(&id, cx)
            }
        }))
}
