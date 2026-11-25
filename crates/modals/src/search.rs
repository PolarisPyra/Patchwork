//! Search: actions, then every chat and task by project, then the projects, filtered by what is
//! typed. Up and Down move through the rows, Enter runs one, and Escape or a press outside closes
//! it.

// Rust guideline compliant 2026-10-09

use gpui::{
    Entity, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton, Role,
    ScrollHandle, SharedString, Window, div, point, prelude::*, px,
};

use patchwork_ui::text::text_area::{Arrow, Changed, Submitted, TextArea};
use patchwork_ui::theme::{font_mono, fs_lg, fs_md, fs_xs, radius, radius_lg, theme};
use patchwork_ui::{icon, overflows, popup_shadow, scrollbar};

/// What a row does when run.
#[derive(Debug, Clone)]
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
    OpenChat(String),
    OpenProject(String),
    /// Runs a command that has a key, by its id.
    Command(&'static str),
    /// Lays the panes out as a ready-made layout, by its place in the list.
    Preset(usize),
    /// Lays the panes out as a layout the user named, by its name.
    Layout(String),
    /// Keeps the panes' layout under a name; picking it asks for the name before saving.
    SaveLayout(String),
    /// Forgets a named layout.
    RemoveLayout(String),
}

const SEARCH_PLACEHOLDER: &str = "Search actions, tasks and projects…";
const NAME_LAYOUT_PLACEHOLDER: &str = "Name this layout…";

/// A row: what it is called, the group it is listed under, a hint at its right, such as its
/// shortcut, and what it does.
#[derive(Debug)]
pub struct Item {
    pub group: String,
    pub label: String,
    pub hint: Option<String>,
    pub action: Action,
}

/// The palette closed: with the row picked, or with none.
#[derive(Debug)]
pub struct Closed(pub Option<Action>);

/// The search field over the rows it filters.
pub struct Palette {
    items: Vec<Item>,
    search: Entity<TextArea>,
    active: usize,
    list_scroll: ScrollHandle,
    /// The keyboard moved the highlight, so the list scrolls to keep it in view.
    follow: bool,
    /// The user picked "Save layout…" and is typing the name for it.
    naming_layout: bool,
}

impl std::fmt::Debug for Palette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Palette")
            .field("items", &self.items.len())
            .field("active", &self.active)
            .field("naming_layout", &self.naming_layout)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<Closed> for Palette {}

impl Palette {
    pub fn new(items: Vec<Item>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            let mut field =
                TextArea::new(SEARCH_PLACEHOLDER, "Search actions, tasks and projects", cx);
            field.single_line = true;
            field.placeholder_color = Some(patchwork_ui::theme::input_placeholder());
            field
        });
        cx.subscribe(&search, |this, _, _: &Changed, cx| {
            this.active = 0;
            this.follow = true;
            cx.notify();
        })
        .detach();
        cx.subscribe(&search, |this, _, arrow: &Arrow, cx| {
            if this.naming_layout {
                return;
            }
            let count = this.rows(cx).len();
            let next = this.active as isize + arrow.0;
            this.active = next.clamp(0, count.saturating_sub(1) as isize) as usize;
            this.follow = true;
            cx.notify();
        })
        .detach();
        cx.subscribe_in(&search, window, |this, _, _: &Submitted, window, cx| {
            if this.naming_layout {
                this.submit_layout_name(cx);
            } else if let Some(&i) = this.rows(cx).get(this.active) {
                this.pick(i, window, cx);
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
            naming_layout: false,
        }
    }

    /// Runs item `i`, or asks for a name when saving the current layout.
    fn pick(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        match &self.items[i].action {
            Action::SaveLayout(_) => self.begin_naming_layout(window, cx),
            action => cx.emit(Closed(Some(action.clone()))),
        }
    }

    /// Switches the field to ask for the layout's name.
    fn begin_naming_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.naming_layout = true;
        self.search.update(cx, |field, cx| {
            field.set_text("", cx);
            field.placeholder = NAME_LAYOUT_PLACEHOLDER.into();
        });
        window.notify_text_input_changed(&self.search.read(cx).focus_handle(cx));
        window.focus(&self.search.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// Leaves layout-naming mode and returns to searching the palette.
    fn cancel_naming_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.naming_layout = false;
        self.active = 0;
        self.follow = true;
        self.search.update(cx, |field, cx| {
            field.set_text("", cx);
            field.placeholder = SEARCH_PLACEHOLDER.into();
        });
        window.notify_text_input_changed(&self.search.read(cx).focus_handle(cx));
        window.focus(&self.search.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// Saves the layout under the name typed, if non-empty.
    fn submit_layout_name(&self, cx: &mut Context<Self>) {
        let name = self.typed(cx);
        if !name.is_empty() {
            cx.emit(Closed(Some(Action::SaveLayout(name))));
        }
    }

    /// What is typed in the search field, trimmed.
    fn typed(&self, cx: &gpui::App) -> String {
        self.search.read(cx).content.trim().to_owned()
    }

    /// The matching items, as indexes, in the order they show: grouped in first-seen order.
    fn rows(&self, cx: &gpui::App) -> Vec<usize> {
        filter_rows(&self.items, &self.search.read(cx).content)
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            if self.naming_layout {
                self.cancel_naming_layout(window, cx);
            } else {
                cx.emit(Closed(None));
            }
            cx.stop_propagation();
        }
    }
}

/// The matching items, as indexes, in the order they show: grouped in first-seen order.
fn filter_rows(items: &[Item], query: &str) -> Vec<usize> {
    let needle = query.trim().to_lowercase();
    let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
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

impl Focusable for Palette {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.search.read(cx).focus_handle(cx)
    }
}

impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let naming = self.naming_layout;
        let list_overflows = !naming && overflows(&self.list_scroll, window, cx);
        let mut children = Vec::new();
        let mut empty_message = None;
        if naming {
            let name = self.typed(cx);
            if name.is_empty() {
                empty_message = Some("Type a name for this layout");
            } else {
                let label = format!("Save layout as \u{201c}{name}\u{201d}");
                children.push(
                    div()
                        .id("palette-save-layout")
                        .role(Role::ListBoxOption)
                        .aria_label(SharedString::from(label.clone()))
                        .aria_selected(true)
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
                        .bg(t.bg_selected)
                        .on_click(cx.listener(|this, _, _, cx| this.submit_layout_name(cx)))
                        .child(div().flex_1().min_w_0().truncate().child(label))
                        .into_any_element(),
                );
            }
        } else {
            let rows = self.rows(cx);
            let active = self.active.min(rows.len().saturating_sub(1));
            // Headers and rows are the list's children in turn, so a row's child index counts the
            // headers above it; a group's first row brings its header into view too.
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
                let label = item.label.clone();
                children.push(
                    div()
                        .id(SharedString::from(format!("palette-{i}")))
                        .role(Role::ListBoxOption)
                        .aria_label(SharedString::from(label.clone()))
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
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.pick(i, window, cx);
                        }))
                        .child(div().flex_1().min_w_0().truncate().child(label))
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
            if rows.is_empty() {
                empty_message = Some("No matches");
            }
            if std::mem::take(&mut self.follow) {
                if active == 0 {
                    self.list_scroll.set_offset(point(px(0.), px(0.)));
                } else {
                    self.list_scroll.scroll_to_item(active_child);
                }
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
                            .child(icon(if naming { "star" } else { "magnifying-glass" }, 18.))
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
                                    .children(empty_message.map(|msg| {
                                        div()
                                            .px(px(12.))
                                            .py(px(24.))
                                            .text_center()
                                            .text_size(px(fs_md()))
                                            .text_color(t.text_subtle)
                                            .child(msg)
                                    }))
                                    .children(children),
                            )
                            .when(!naming, |box_el| {
                                box_el.children(scrollbar(
                                    &self.list_scroll,
                                    10.,
                                    2.,
                                    t.border_strong,
                                ))
                            }),
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
                            .when(!naming, |bar| {
                                bar.child("↑↓ navigate").child("↵ run").child("esc close")
                            })
                            .when(naming, |bar| bar.child("↵ save").child("esc back")),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Item, filter_rows};

    fn sample_items() -> Vec<Item> {
        vec![
            Item {
                group: "Actions".into(),
                label: "New chat".into(),
                hint: None,
                action: Action::NewChat,
            },
            Item {
                group: "Panes".into(),
                label: "Layout: Grid of four".into(),
                hint: None,
                action: Action::Preset(4),
            },
            Item {
                group: "Panes".into(),
                label: "Save layout…".into(),
                hint: None,
                action: Action::SaveLayout(String::new()),
            },
        ]
    }

    #[test]
    fn save_layout_only_matches_its_own_label() {
        let items = sample_items();
        assert_eq!(filter_rows(&items, ""), vec![0, 1, 2]);
        assert_eq!(filter_rows(&items, "save"), vec![2]);
        assert_eq!(filter_rows(&items, "layout"), vec![1, 2]);
        assert!(filter_rows(&items, "unrelated").is_empty());
    }
}
