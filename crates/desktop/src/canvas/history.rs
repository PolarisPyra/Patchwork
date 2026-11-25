//! The open file's past: who last changed each line, beside the code, and the list of commits
//! that changed the file, each opening what it changed.

// Rust guideline compliant 2026-10-09

use std::rc::Rc;

use gpui::{AnyElement, FontWeight, Role, SharedString, Stateful, div, prelude::*, px};
use patchwork_core::source_control::{self as ws, BlameRange, FileRevision};

use super::body::centered;
use super::open_file::{Canvas, CanvasTab};
use patchwork_ui::theme::{Theme, font_ui, fs_sm, radius};
use patchwork_ui::{format_date, icon, icon_button, spinner, tooltip};

/// How many of a file's commits are read at once.
const PAGE: usize = 100;

/// The width of the column that names each line's commit.
const BLAME_WIDTH: f32 = 230.;

/// How many characters of a hash are shown.
const SHORT_HASH: usize = 7;

/// A hash as it is shown.
fn short(hash: &str) -> &str {
    hash.get(..SHORT_HASH).unwrap_or(hash)
}

impl Canvas {
    /// Shows or hides who last changed each line, reading it the first time.
    pub(super) fn toggle_blame(&mut self, cx: &mut Context<Self>) {
        self.blame_on = !self.blame_on;
        if self.blame_on && self.blame.is_none() {
            self.load_blame(cx);
        }
        cx.notify();
    }

    /// Reads who last changed each line of the file, off the main thread.
    pub(super) fn load_blame(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let (root, seq) = (self.root.clone(), self.load_seq);
        let task =
            cx.background_spawn(
                async move { ws::git_blame(&root, &path).map_err(|e| e.to_string()) },
            );
        cx.spawn(async move |this, cx| {
            let blame = task.await;
            this.update(cx, |this, cx| {
                if this.load_seq == seq {
                    this.blame = Some(blame.map(Rc::new));
                    cx.notify();
                }
            })
        })
        .detach();
    }

    /// Reads the commits that changed the file, off the main thread: the first of them, or with
    /// `more` the ones after those already listed.
    pub(super) fn load_history(&mut self, more: bool, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let skip = match (&self.history, more) {
            (Some(Ok(listed)), true) => listed.len(),
            _ => 0,
        };
        let (root, seq) = (self.root.clone(), self.load_seq);
        patchwork_ui::off_thread(
            cx,
            move || ws::file_log(&root, &path, skip, PAGE).map_err(|e| e.to_string()),
            move |this, read, cx| {
                if this.load_seq != seq {
                    return;
                }
                this.history_more = read.as_ref().is_ok_and(|page| page.len() == PAGE);
                this.history = Some(match (this.history.take(), read) {
                    (Some(Ok(mut listed)), Ok(page)) if skip > 0 => {
                        listed.extend(page);
                        Ok(listed)
                    }
                    (_, read) => read,
                });
                cx.notify();
            },
        );
    }

    /// Shows what `revision` changed in the file, in place of its uncommitted changes.
    pub(super) fn show_revision(&mut self, revision: FileRevision, cx: &mut Context<Self>) {
        self.revision = Some(revision);
        self.tab = CanvasTab::Diff;
        self.diff = None;
        self.load(cx);
        cx.notify();
    }

    /// Goes back to the file's uncommitted changes.
    pub(super) fn clear_revision(&mut self, cx: &mut Context<Self>) {
        self.revision = None;
        self.diff = None;
        self.load(cx);
        cx.notify();
    }

    /// The run of lines line `i` is in, once the blame is read.
    fn blame_at(&self, i: usize) -> Option<&BlameRange> {
        let ranges = self.blame.as_ref()?.as_ref().ok()?;
        let at = ranges.partition_point(|r| r.start <= i).checked_sub(1)?;
        ranges.get(at).filter(|r| i < r.start + r.lines)
    }

    /// The cell left of line `i` that names the commit that last changed it, on the first line of
    /// each run. Pressing it shows what that commit changed in the file.
    pub(super) fn blame_cell(
        &self,
        i: usize,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<gpui::Div>> {
        if !self.blame_on {
            return None;
        }
        let cell = div()
            .id(("blame", i))
            .flex_none()
            .w(px(BLAME_WIDTH))
            .pl(px(12.))
            .pr(px(8.))
            .truncate()
            .font_family(font_ui())
            .text_size(px(fs_sm()))
            .text_color(t.text_subtle);
        let Some(range) = self.blame_at(i).filter(|r| r.start == i) else {
            return Some(cell);
        };
        if range.hash.is_empty() {
            return Some(cell.child("Not committed yet"));
        }
        let date = format_date(range.time);
        let day = date.rsplit_once(", ").map_or(date.as_str(), |(day, _)| day);
        let about = format!(
            "{} {}\n{}, {date}",
            short(&range.hash),
            range.subject,
            range.author
        );
        let revision = FileRevision {
            hash: range.hash.clone(),
            author: range.author.clone(),
            time: range.time,
            subject: range.subject.clone(),
            path: self.path.clone().unwrap_or_default(),
        };
        let (fg, last) = (t.text, range.start + range.lines);
        Some(
            cell.role(Role::Button)
                .aria_label(format!(
                    "Lines {} to {last}: {} by {}, {date}. Show the commit's changes",
                    i + 1,
                    range.subject,
                    range.author
                ))
                .tab_index(0)
                .cursor_pointer()
                .hover(move |s| s.text_color(fg))
                .focus(move |s| s.text_color(fg))
                .tooltip(tooltip(about))
                .child(format!("{} · {day}", range.author))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.show_revision(revision.clone(), cx);
                })),
        )
    }

    /// The commit whose changes the diff shows, with the way back to the uncommitted ones.
    pub(super) fn revision_bar(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let revision = self.revision.as_ref()?;
        Some(
            div()
                .id("canvas-revision")
                .role(Role::Status)
                .aria_label(format!(
                    "Showing commit {}: {}",
                    short(&revision.hash),
                    revision.subject
                ))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .py(px(6.))
                .px(px(20.))
                .border_b_1()
                .border_color(t.border)
                .bg(t.bg_panel)
                .text_size(px(fs_sm()))
                .child(icon("git-commit", 14.).text_color(t.text_muted))
                .child(
                    div()
                        .flex_none()
                        .text_color(t.text_muted)
                        .child(short(&revision.hash).to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(revision.subject.clone()),
                )
                .child(
                    icon_button("close-revision", "Show uncommitted changes", "x", 14., cx)
                        .tooltip(tooltip("Show uncommitted changes"))
                        .on_click(cx.listener(|this, _, _, cx| this.clear_revision(cx))),
                )
                .into_any_element(),
        )
    }

    /// The commits that changed the file, newest first.
    pub(super) fn history_view(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let revisions = match &self.history {
            None => {
                return centered(div())
                    .id("canvas-history-loading")
                    .role(Role::Status)
                    .aria_label("Loading the file's history")
                    .child(spinner(18., t.text_subtle))
                    .into_any_element();
            }
            Some(Err(e)) => {
                return centered(div())
                    .id("canvas-history-error")
                    .role(Role::Alert)
                    .text_color(t.text_muted)
                    .child(e.clone())
                    .into_any_element();
            }
            Some(Ok(revisions)) if revisions.is_empty() => {
                return centered(div())
                    .text_color(t.text_muted)
                    .child("No commit has changed this file.")
                    .into_any_element();
            }
            Some(Ok(revisions)) => revisions,
        };
        let shown = self.revision.as_ref().map(|r| r.hash.as_str());
        let (hover, selected) = (t.bg_hover, t.bg_selected_muted);
        let rows = revisions.iter().enumerate().map(|(i, revision)| {
            let on = shown == Some(revision.hash.as_str());
            let date = format_date(revision.time);
            let open = revision.clone();
            div()
                .id(("revision", i))
                .role(Role::ListItem)
                .aria_label(format!(
                    "{}, {} by {}, {date}",
                    revision.subject,
                    short(&revision.hash),
                    revision.author
                ))
                .aria_selected(on)
                .tab_index(0)
                .flex()
                .flex_col()
                .gap(px(2.))
                .py(px(6.))
                .px(px(10.))
                .rounded(px(radius()))
                .cursor_pointer()
                .when(on, |d| d.bg(selected))
                .hover(move |s| s.bg(hover))
                .focus(move |s| s.bg(hover))
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(revision.subject.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(fs_sm()))
                        .text_color(t.text_muted)
                        .child(format!(
                            "{} · {} · {date}",
                            short(&revision.hash),
                            revision.author
                        )),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.show_revision(open.clone(), cx);
                }))
        });
        div()
            .id("canvas-history")
            .role(Role::List)
            .aria_label("Commits that changed this file")
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(12.))
            .children(rows)
            .when(self.history_more, |d| {
                d.child(
                    patchwork_ui::button("more-history", SharedString::from("Show older commits"))
                        .mt(px(6.))
                        .py(px(6.))
                        .px(px(10.))
                        .rounded(px(radius()))
                        .text_color(t.text_muted)
                        .hover(move |s| s.bg(hover))
                        .child("Show older commits")
                        .on_click(cx.listener(|this, _, _, cx| this.load_history(true, cx))),
                )
            })
            .into_any_element()
    }
}
