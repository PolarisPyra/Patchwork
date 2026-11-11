//! The changed files as a list that draws only the rows in view: a project can have hundreds of
//! changes, and laying every row out again made each redraw of the window slow, a resize most
//! of all.

// Rust guideline compliant 2026-10-09

use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

use gpui::{AnyElement, SharedString, Window, div, prelude::*, px};
use patchwork_server::workspace::FileChange;

use super::rows::{group_button, group_header, row_action};
use super::state::{ChangesView, FS_SM, FS_XS};
use crate::ui::theme::{RADIUS_SM, theme};
use crate::ui::{base_name, button, icon, spinner, tooltip};

/// A group of changes, which decides the buttons on its header.
#[derive(Clone)]
pub(super) enum Group {
    /// Files with merge conflicts, which the agent can resolve.
    Merge,
    /// The files the open reply changed.
    Reply,
    Staged,
    /// The changes not staged, by path, which its header discards or stages at once.
    Unstaged(Vec<String>),
}

/// One row of the list.
#[derive(Clone)]
pub(super) enum Entry {
    /// The commit box, and the pull request bar under it.
    Top,
    Header {
        title: &'static str,
        count: usize,
        group: Group,
    },
    Change(FileChange),
    /// A file the open reply changed that has no change now.
    ReplyFile(String),
    /// The space under the last row.
    End,
}

impl Entry {
    /// What the entry's height depends on, and what tells it from the entry it replaced.
    fn shape(&self, state: &mut DefaultHasher) {
        match self {
            Entry::Top => 0.hash(state),
            Entry::Header { title, count, .. } => (1, title, count).hash(state),
            Entry::Change(c) => (2, &c.path, &c.status, c.staged).hash(state),
            Entry::ReplyFile(path) => (3, path).hash(state),
            Entry::End => 4.hash(state),
        }
    }
}

impl ChangesView {
    /// Sets the entries the list draws. Once they change, the list measures them again, keeping
    /// its place; the top entry, which grows with the commit message, is measured each time.
    pub(super) fn set_entries(&mut self, entries: Vec<Entry>) {
        let mut state = DefaultHasher::new();
        for entry in &entries {
            entry.shape(&mut state);
        }
        let shape = state.finish();
        if shape != self.list_shape {
            self.list_shape = shape;
            let (old, new) = (self.entries.len(), entries.len());
            if new > old {
                self.list.splice(old..old, new - old);
            } else if new < old {
                self.list.splice(new..old, 0);
            }
            self.list.remeasure();
        } else if matches!(entries.first(), Some(Entry::Top)) {
            self.list.remeasure_items(0..1);
        }
        self.entries = Rc::new(entries);
    }

    /// The entry at `ix`, drawn as wide as the list: the list lays each entry out on its own,
    /// and one left to size itself can grow past the list and be cut off.
    pub(super) fn entry(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .w_full()
            .child(self.entry_content(ix, window, cx))
            .into_any_element()
    }

    fn entry_content(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *theme(cx);
        let Some(entry) = self.entries.get(ix).cloned() else {
            return div().into_any_element();
        };
        match entry {
            Entry::Top => div()
                .flex()
                .flex_col()
                .child(self.commit_box(self.has_changes, &t, window, cx))
                .children(
                    self.sync
                        .as_ref()
                        .and_then(|s| s.pull_request.clone())
                        .map(|pr| self.pull_request_bar(&pr, &t, cx)),
                )
                .into_any_element(),
            Entry::Header {
                title,
                count,
                group,
            } => group_header(title, count, self.header_actions(&group, cx), &t).into_any_element(),
            Entry::Change(c) => div()
                .px(px(4.))
                .child(self.change_row(&c, self.full_access, &t, cx))
                .into_any_element(),
            Entry::ReplyFile(path) => div()
                .px(px(4.))
                .child(self.reply_file(&path, cx))
                .into_any_element(),
            Entry::End => div().h(px(4.)).into_any_element(),
        }
    }

    /// The buttons on a group's header.
    fn header_actions(&self, group: &Group, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let t = *theme(cx);
        match group {
            Group::Merge => {
                let lead = if self.resolving {
                    spinner(12., t.text_subtle).into_any_element()
                } else {
                    icon("sparkle", 14.).into_any_element()
                };
                vec![
                    group_button("resolve", "Resolve with agent", lead, &t)
                        .on_click(cx.listener(|this, _, _, cx| {
                            if !this.resolving {
                                this.resolve_with_agent(cx);
                            }
                        }))
                        .into_any_element(),
                ]
            }
            Group::Reply => vec![
                group_button(
                    "back-to-project",
                    "Project",
                    icon("arrow-left", 14.).into_any_element(),
                    &t,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(w) = this.workspace.upgrade() {
                        w.update(cx, |w, cx| {
                            w.change_focus = None;
                            cx.notify();
                        });
                    }
                    this.missing.clear();
                }))
                .into_any_element(),
            ],
            Group::Staged => Vec::new(),
            Group::Unstaged(_) if self.full_access => Vec::new(),
            Group::Unstaged(paths) => {
                let name = SharedString::from("group:Changes");
                let (discard, stage) = (paths.clone(), paths.clone());
                vec![
                    row_action(
                        "discard-all",
                        "Discard all changes",
                        "arrow-counter-clockwise",
                        name.clone(),
                        &t,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.discard(discard.clone(), window, cx);
                    }))
                    .into_any_element(),
                    row_action("stage-all", "Stage all changes", "plus", name, &t)
                        .on_click(cx.listener(move |this, _, _, cx| this.stage(stage.clone(), cx)))
                        .into_any_element(),
                ]
            }
        }
    }

    /// A file the open reply changed that has no change now: deleted since, or changed back.
    fn reply_file(&self, path: &str, cx: &mut Context<Self>) -> AnyElement {
        let t = *theme(cx);
        let hover = t.bg_hover;
        let missing = self.missing.contains(path);
        let open = path.to_string();
        let workspace = self.workspace.clone();
        button(
            SharedString::from(format!("reply-file:{path}")),
            path.to_string(),
        )
        .h(px(22.))
        .gap(px(6.))
        .rounded(px(RADIUS_SM))
        .pr(px(6.))
        .pl(px(20.))
        .text_size(px(FS_SM))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover))
        .tooltip(tooltip(path.to_string()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(missing, gpui::Styled::line_through)
                .child(base_name(path).to_string()),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(FS_XS))
                .text_color(t.text_subtle)
                .child(if missing { "Deleted" } else { "No changes" }),
        )
        .on_click(move |_, window, cx| {
            let open = open.clone();
            let _ = workspace.update(cx, |w, cx| w.open_change(&open, window, cx));
        })
        .into_any_element()
    }
}
