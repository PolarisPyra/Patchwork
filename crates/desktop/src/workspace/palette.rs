//! Search: actions, then every chat and task by project, then the projects, filtered by what is
//! typed. Up and Down move through the rows, Enter runs one, and Escape or a press outside closes
//! it.

// Rust guideline compliant 2026-10-09

use gpui::{
    Entity, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton, Role,
    ScrollHandle, SharedString, Window, div, point, prelude::*, px,
};

use patchwork_server::threads::{MessageStatus, Role as MessageRole};

use crate::ui::text_area::{Arrow, Changed, Submitted, TextArea};
use crate::settings::shortcuts::key_label;
use crate::ui::theme::{font_mono, fs_lg, fs_md, fs_xs, radius, radius_lg, theme};
use crate::ui::{display_title, icon, overflows, popup_shadow, scrollbar};
use crate::workspace::Workspace;

/// What a row does when run.
#[derive(Clone)]
pub enum Action {
    NewChat,
    NewTask(String),
    NewWorktreeTask(String),
    /// Runs the checks for the reply, by its id.
    RunChecks(String),
    ChatBackground(String),
    ToggleTheme,
    OpenSettings,
    OpenFolder,
    OpenThread(String),
    OpenProject(String),
}

pub struct Item {
    group: String,
    label: String,
    hint: Option<String>,
    action: Action,
}

/// The palette closed: with the row picked, or with none.
pub struct Closed(pub Option<Action>);

pub struct Palette {
    items: Vec<Item>,
    search: Entity<TextArea>,
    active: usize,
    list_scroll: ScrollHandle,
    /// The keyboard moved the highlight, so the list scrolls to keep it in view.
    follow: bool,
}

impl EventEmitter<Closed> for Palette {}

/// What search lists: actions, then every chat and task by project, then the projects.
pub fn items(w: &Workspace) -> Vec<Item> {
    let item = |group: &str, label: String, hint: Option<String>, action| Item {
        group: group.into(),
        label,
        hint,
        action,
    };
    let new_key = key_label(&w.server, "chat.new");
    let project = w.project();
    let mut items = vec![item(
        "Actions",
        "New chat".into(),
        new_key.clone().filter(|_| project.is_none()),
        Action::NewChat,
    )];
    if let Some(project) = project {
        items.push(item(
            "Actions",
            format!("New task in {}", project.name),
            new_key,
            Action::NewTask(project.id.clone()),
        ));
        items.push(item(
            "Actions",
            format!("New task in a worktree of {}", project.name),
            None,
            Action::NewWorktreeTask(project.id.clone()),
        ));
    }
    let thread = w.thread.as_ref();
    // The reply the checks are for: the last one the open task got.
    let checkable = thread.filter(|_| project.is_some()).and_then(|t| {
        t.messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::Assistant && m.status != MessageStatus::Streaming)
    });
    if let Some(reply) = checkable {
        items.push(item(
            "Actions",
            "Run checks for the open task".into(),
            None,
            Action::RunChecks(reply.id.clone()),
        ));
    }
    if let Some(thread) = thread.filter(|t| t.parent.is_none()) {
        items.push(item(
            "Actions",
            "Change this chat’s background".into(),
            None,
            Action::ChatBackground(thread.id.clone()),
        ));
    }
    items.push(item(
        "Actions",
        "Toggle theme".into(),
        None,
        Action::ToggleTheme,
    ));
    items.push(item(
        "Actions",
        "Open settings".into(),
        key_label(&w.server, "settings.open"),
        Action::OpenSettings,
    ));
    items.push(item(
        "Actions",
        "Open folder…".into(),
        None,
        Action::OpenFolder,
    ));
    let projects = &w.settings.projects;
    items.extend(
        w.threads
            .iter()
            .filter(|t| t.archived_at.is_none() && t.parent.is_none())
            .map(|t| {
                let group = t
                    .project_id
                    .as_ref()
                    .and_then(|id| projects.iter().find(|p| &p.id == id))
                    .map_or("Chats", |p| p.name.as_str());
                item(
                    group,
                    display_title(&t.title),
                    None,
                    Action::OpenThread(t.id.clone()),
                )
            }),
    );
    items.extend(projects.iter().map(|p| {
        item(
            "Projects",
            p.name.clone(),
            Some(p.path.clone()),
            Action::OpenProject(p.id.clone()),
        )
    }));
    items
}

impl Palette {
    pub fn new(items: Vec<Item>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let t = *theme(cx);
        let search = cx.new(|cx| {
            let mut field = TextArea::new(
                "Search actions, tasks and projects…",
                "Search actions, tasks and projects",
                cx,
            );
            field.single_line = true;
            field.text_color = t.text;
            field.placeholder_color = crate::ui::theme::input_placeholder();
            field.selection_color = t.accent.opacity(0.3);
            field
        });
        cx.subscribe(&search, |this, _, _: &Changed, cx| {
            this.active = 0;
            this.follow = true;
            cx.notify();
        })
        .detach();
        cx.subscribe(&search, |this, _, arrow: &Arrow, cx| {
            let count = this.rows(cx).len();
            let next = this.active as isize + arrow.0;
            this.active = next.clamp(0, count.saturating_sub(1) as isize) as usize;
            this.follow = true;
            cx.notify();
        })
        .detach();
        cx.subscribe(&search, |this, _, _: &Submitted, cx| {
            let picked = this
                .rows(cx)
                .get(this.active)
                .map(|&i| this.items[i].action.clone());
            if picked.is_some() {
                cx.emit(Closed(picked));
            }
        })
        .detach();
        window.focus(&search.read(cx).focus_handle(cx), cx);
        Self {
            items,
            search,
            active: 0,
            list_scroll: ScrollHandle::new(),
            follow: false,
        }
    }

    /// The matching items, as indexes, in the order they show: grouped in first-seen order.
    fn rows(&self, cx: &gpui::App) -> Vec<usize> {
        let needle = self.search.read(cx).content.trim().to_lowercase();
        let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
        for (i, item) in self.items.iter().enumerate() {
            if !item.label.to_lowercase().contains(&needle) {
                continue;
            }
            match groups.iter_mut().find(|(name, _)| *name == item.group) {
                Some((_, rows)) => rows.push(i),
                None => groups.push((&item.group, vec![i])),
            }
        }
        groups.into_iter().flat_map(|(_, rows)| rows).collect()
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.emit(Closed(None));
            cx.stop_propagation();
        }
    }
}

impl Focusable for Palette {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.search.read(cx).focus_handle(cx)
    }
}

impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let list_overflows = overflows(&self.list_scroll, window, cx);
        let rows = self.rows(cx);
        let active = self.active.min(rows.len().saturating_sub(1));
        // Headers and rows are the list's children in turn, so a row's child index counts the
        // headers above it; a group's first row brings its header into view too.
        let mut children = Vec::new();
        let mut last_group: Option<&str> = None;
        let mut active_child = 0;
        for (n, &i) in rows.iter().enumerate() {
            let item = &self.items[i];
            if last_group != Some(item.group.as_str()) {
                last_group = Some(&item.group);
                if n == active {
                    active_child = children.len();
                }
                children.push(
                    div()
                        .px(px(12.))
                        .pt(px(12.))
                        .pb(px(6.))
                        .text_size(px(fs_xs()))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.text_subtle)
                        .child(item.group.to_uppercase())
                        .into_any_element(),
                );
            } else if n == active {
                active_child = children.len();
            }
            let is_active = n == active;
            let action = item.action.clone();
            children.push(
                div()
                    .id(SharedString::from(format!("palette-{i}")))
                    .role(Role::ListBoxOption)
                    .aria_label(SharedString::from(item.label.clone()))
                    .aria_selected(is_active)
                    .flex()
                    .flex_none()
                    .h(px(36.))
                    .w_full()
                    .items_center()
                    .gap(px(12.))
                    .px(px(12.))
                    .rounded(px(radius()))
                    .cursor_pointer()
                    .text_size(px(fs_md()))
                    .when(is_active, |row| row.bg(t.bg_selected))
                    .when(!is_active, |row| row.hover(|s| s.bg(t.bg_hover)))
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.active != n {
                            this.active = n;
                            this.follow = false;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(Closed(Some(action.clone())))))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(item.label.clone()),
                    )
                    .children(item.hint.clone().map(|hint| {
                        div()
                            .flex_none()
                            .max_w(px(220.))
                            .truncate()
                            .font_family(font_mono())
                            .text_size(px(fs_xs()))
                            .text_color(if is_active {
                                t.text_muted
                            } else {
                                t.text_subtle
                            })
                            .child(hint)
                    }))
                    .into_any_element(),
            );
        }
        if std::mem::take(&mut self.follow) {
            if active == 0 {
                self.list_scroll.set_offset(point(px(0.), px(0.)));
            } else {
                self.list_scroll.scroll_to_item(active_child);
            }
        }
        let viewport = window.viewport_size();
        div()
            .id("palette-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_start()
            .justify_center()
            .pt(viewport.height * 0.15)
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Closed(None))),
            )
            .on_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .id("palette")
                    .role(Role::Dialog)
                    .aria_label("Command palette")
                    .flex()
                    .flex_col()
                    .w(px(600.))
                    .max_w(viewport.width - px(32.))
                    .max_h(viewport.height * 0.6)
                    .overflow_hidden()
                    .rounded(px(radius_lg()))
                    .border_1()
                    .border_color(t.border_strong)
                    .bg(t.bg_elevated)
                    .shadow(popup_shadow(&t))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .h(px(56.))
                            .items_center()
                            .gap(px(12.))
                            .px(px(20.))
                            .border_b_1()
                            .border_color(t.border)
                            .text_color(t.text_subtle)
                            .child(icon("magnifying-glass", 18.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(fs_lg()))
                                    .child(self.search.clone()),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .child(
                                div()
                                    .id("palette-results")
                                    .role(Role::ListBox)
                                    .aria_label("Results")
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h_0()
                                    .p(px(8.))
                                    // And the web list's 10px scrollbar when there is one.
                                    .when(list_overflows, |list| list.pr(px(18.)))
                                    .overflow_y_scroll()
                                    .track_scroll(&self.list_scroll)
                                    .when(rows.is_empty(), |list| {
                                        list.child(
                                            div()
                                                .px(px(12.))
                                                .py(px(24.))
                                                .text_center()
                                                .text_size(px(fs_md()))
                                                .text_color(t.text_subtle)
                                                .child("No matches"),
                                        )
                                    })
                                    .children(children),
                            )
                            .children(scrollbar(&self.list_scroll, 10., 2., t.border_strong)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .h(px(36.))
                            .items_center()
                            .gap(px(16.))
                            .px(px(16.))
                            .border_t_1()
                            .border_color(t.border)
                            .text_size(px(fs_xs()))
                            .text_color(t.text_subtle)
                            .child("↑↓ navigate")
                            .child("↵ run")
                            .child("esc close"),
                    ),
            )
    }
}
