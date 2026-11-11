//! Source Control drawn: its panes, with the commit box and the changed files in them.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, Div, FontWeight, MouseButton, MouseDownEvent, Role, SharedString, Stateful, Window,
    div, list, prelude::*, px,
};
use patchwork_server::threads::AccessMode;
use patchwork_server::workspace::FileChange;

use crate::sidebar::graph;
use crate::ui::menu::Menu;
use crate::ui::theme::{RADIUS_SM, Theme, theme};
use crate::ui::{button, icon, list_scrollbar, scrollbar, tooltip};
use super::list::{Entry, Group};
use super::rows::reply_files;
use super::state::{ChangesView, FS_SM, FS_XS, PANE_MIN};

impl ChangesView {
    /// A collapsible part of the view: its header, and its body when open.
    #[expect(
        clippy::too_many_arguments,
        reason = "each argument is a separate piece of what it draws"
    )]
    fn pane(
        &self,
        title: &'static str,
        open: bool,
        height: Option<f32>,
        actions: Option<AnyElement>,
        body: AnyElement,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let caret = if open { "caret-down" } else { "caret-right" };
        div()
            .id(title)
            .role(Role::Region)
            .aria_label(title)
            .flex()
            .flex_col()
            .min_h_0()
            .when(title == "Graph", |d| d.border_t_1().border_color(t.border))
            .map(|d| match (open, height) {
                (true, Some(h)) => d.h(px(h)).flex_none(),
                (true, None) => d.flex_1(),
                _ => d.flex_none(),
            })
            .child(
                div()
                    .h(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .pr(px(8.))
                    .child(
                        button(SharedString::from(format!("pane:{title}")), title)
                            .flex_1()
                            .min_w_0()
                            .gap(px(4.))
                            .px(px(4.))
                            .text_size(px(FS_XS))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text)
                            .aria_expanded(open)
                            .child(icon(caret, 12.))
                            .child(title)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if title == "Graph" {
                                    this.graph_open = !this.graph_open;
                                } else {
                                    this.changes_open = !this.changes_open;
                                }
                                cx.notify();
                            })),
                    )
                    .children(actions.filter(|_| open)),
            )
            .when(open, |d| d.child(body))
    }
}

impl Render for ChangesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let Some(workspace) = self.workspace.upgrade() else {
            return div().into_any_element();
        };
        let w = workspace.read(cx);
        let changes = w.changes.clone();
        let has_project = w.project().is_some();
        let full_access = w
            .thread
            .as_ref()
            .is_some_and(|th| th.access_mode == AccessMode::FullAccess);
        let replies = reply_files(w);
        let live: Vec<_> = w
            .threads
            .iter()
            .filter(|th| th.archived_at.is_none() && th.parent.is_none())
            .cloned()
            .collect();
        let archived: Vec<_> = w
            .threads
            .iter()
            .filter(|th| th.archived_at.is_some() && th.parent.is_none())
            .cloned()
            .collect();
        let has_changes = !changes.is_empty();

        let conflicted: Vec<&FileChange> = changes
            .iter()
            .filter(|c| c.status == "conflicted")
            .collect();
        let pinned: Vec<&FileChange> = changes
            .iter()
            .filter(|c| replies.contains(&c.path) && c.status != "conflicted")
            .collect();
        let rest: Vec<&FileChange> = changes
            .iter()
            .filter(|c| !replies.contains(&c.path) && c.status != "conflicted")
            .collect();
        let staged: Vec<&FileChange> = rest.iter().copied().filter(|c| c.staged).collect();
        let unstaged: Vec<&FileChange> = rest.iter().copied().filter(|c| !c.staged).collect();

        let mut entries = Vec::new();
        if has_project {
            entries.push(Entry::Top);
        }
        if !conflicted.is_empty() {
            entries.push(Entry::Header {
                title: "Merge Changes",
                count: conflicted.len(),
                group: Group::Merge,
            });
            entries.extend(conflicted.iter().map(|c| Entry::Change((*c).clone())));
        }
        if !replies.is_empty() {
            entries.push(Entry::Header {
                title: "From this reply",
                count: replies.len(),
                group: Group::Reply,
            });
            entries.extend(pinned.iter().map(|c| Entry::Change((*c).clone())));
            entries.extend(
                replies
                    .iter()
                    .filter(|p| !changes.iter().any(|c| &c.path == *p))
                    .map(|p| Entry::ReplyFile(p.clone())),
            );
        }
        if !staged.is_empty() {
            entries.push(Entry::Header {
                title: "Staged Changes",
                count: staged.len(),
                group: Group::Staged,
            });
            entries.extend(staged.iter().map(|c| Entry::Change((*c).clone())));
        }
        if !unstaged.is_empty() {
            entries.push(Entry::Header {
                title: "Changes",
                count: unstaged.len(),
                group: Group::Unstaged(unstaged.iter().map(|c| c.path.clone()).collect()),
            });
            entries.extend(unstaged.iter().map(|c| Entry::Change((*c).clone())));
        }
        entries.push(Entry::End);
        self.full_access = full_access;
        self.has_changes = has_changes;
        self.set_entries(entries);

        let changes_body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                list(
                    self.list.clone(),
                    cx.processor(|this, ix: usize, window, cx| this.entry(ix, window, cx)),
                )
                .size_full(),
            )
            .children(list_scrollbar(&self.list, 4., 0., t.border_strong))
            .into_any_element();
        let graph_body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("graph-scroll")
                    .track_scroll(&self.graph_scroll)
                    .overflow_y_scroll()
                    .size_full()
                    .pb(px(4.))
                    .when(has_project, |d| {
                        d.child(graph::section(self, &live, &archived, &t, cx))
                    }),
            )
            .children(scrollbar(&self.graph_scroll, 4., 0., t.border_strong))
            .into_any_element();

        let (fg, hover) = (t.text, t.bg_hover);
        let graph_height = self.changes_open.then_some(self.graph_height);
        let divider = (self.changes_open && self.graph_open).then(|| {
            let height = self.graph_height;
            div()
                .id("graph-resize")
                .role(Role::Splitter)
                .aria_orientation(gpui::Orientation::Horizontal)
                .aria_label("Resize graph")
                .aria_numeric_value(f64::from(height))
                .aria_min_numeric_value(f64::from(PANE_MIN))
                .tab_index(0)
                .relative()
                .h(px(4.))
                .mb(px(-4.))
                .flex_none()
                .cursor_row_resize()
                .focus(|s| s.bg(t.accent))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                        this.resizing = Some((f32::from(e.position.y), this.graph_height));
                        cx.stop_propagation();
                    }),
                )
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                    let step = match e.keystroke.key.as_str() {
                        "up" => 16.,
                        "down" => -16.,
                        _ => return,
                    };
                    let total = f32::from(window.viewport_size().height);
                    this.set_graph_height(this.graph_height + step, total);
                    cx.notify();
                }))
        });

        let menu = self.menu.as_ref().map(|(m, at, _)| Menu::view(m, *at));
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .mt(px(6.))
                    .h(px(32.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(12.))
                    .text_size(px(FS_SM))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Source Control")
                    .child(
                        button("refresh-changes", "Refresh")
                            .size(px(28.))
                            .justify_center()
                            .rounded(px(RADIUS_SM))
                            .text_color(t.text_muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                            .tooltip(tooltip("Refresh"))
                            .child(icon("arrows-clockwise", 18.).mt(px(1.)))
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_all(cx))),
                    ),
            )
            .child(
                div()
                    .id("panes")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .on_mouse_move(cx.listener(
                        move |this, e: &gpui::MouseMoveEvent, window, cx| {
                            if let Some((start, height)) = this.resizing {
                                let total = f32::from(window.viewport_size().height);
                                this.set_graph_height(
                                    height + start - f32::from(e.position.y),
                                    total,
                                );
                                cx.notify();
                            }
                        },
                    ))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            if this.resizing.take().is_some() {
                                cx.notify();
                            }
                        }),
                    )
                    .child(self.pane(
                        "Changes",
                        self.changes_open,
                        None,
                        None,
                        changes_body,
                        &t,
                        cx,
                    ))
                    .children(divider)
                    .child(self.pane(
                        "Graph",
                        self.graph_open,
                        graph_height,
                        None,
                        graph_body,
                        &t,
                        cx,
                    )),
            )
            .children(menu)
            .into_any_element()
    }
}
