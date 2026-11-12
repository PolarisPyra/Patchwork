//! The diff drawn: its hunks, and the notes and their editor beside the lines.

// Rust guideline compliant 2026-10-09

use super::review::Draft;
use super::review::note_for;
use super::parse::Row;
use super::review::anchored_at;
use super::parse::Kind;
use super::review::Review;
use super::review::ReviewNote;
use std::hash::{DefaultHasher, Hash, Hasher};

use gpui::{
    AnyElement, App, Div, Focusable, FontWeight, Role, SharedString, Window, div, list, prelude::*,
    px, relative,
};
use patchwork_server::workspace::HunkAction;
use crate::canvas::Canvas;
use crate::ui::theme::{FONT_MONO, FS_SM, FS_XS, RADIUS, RADIUS_SM, Theme, theme};
use crate::ui::{BtnKind, btn, button, icon, tooltip};

/// A monospace character's width at `--fs-md`, for the widest line's floor.
pub(super) const CHAR_W: f32 = 7.8;

impl Canvas {
    /// The diff of the open file, as a list that draws only the lines in view: a diff can run
    /// to thousands of lines, and drawing them all made every frame of a window resize slow.
    /// Item 0 holds the notes whose lines have left the diff; item `i` holds row `i - 1`, with
    /// its notes and the open note editor under it.
    pub(in crate::canvas) fn diff_view(
        &mut self,
        path: &str,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(parsed) = self.diff_rows.clone().filter(|p| !p.rows.is_empty()) else {
            return crate::canvas::body::centered(div())
                .text_color(t.text_muted)
                .child("No changes in this file.")
                .into_any_element();
        };
        // A line's height changes with the notes under it and the editor, which are changed from
        // several places, so the list measures its lines again when any of them has.
        let notes = self.notes_on(path, cx);
        let mut shape = DefaultHasher::new();
        for note in &notes {
            (note.id, &note.place, &note.text).hash(&mut shape);
        }
        self.draft.as_ref().map(|d| d.row).hash(&mut shape);
        self.editing.hash(&mut shape);
        let shape = shape.finish();
        if shape != self.diff_shape {
            self.diff_shape = shape;
            self.diff_list.remeasure();
        }
        let path = path.to_string();
        let last = parsed.rows.len();
        let rows = list(
            self.diff_list.clone(),
            // As wide as the list: the list lays each item out on its own, and one left to size
            // itself can stop short of the list's width.
            cx.processor(move |this, item: usize, window, cx| {
                div()
                    .w_full()
                    .child(this.diff_item(&path, item, last, window, cx))
                    .into_any_element()
            }),
        )
        .w(px(parsed.widest))
        .min_w_full()
        .h_full();
        div()
            .id("diff")
            .size_full()
            .overflow_x_scroll()
            // The list scrolls down on its own; without this the wheel also scrolls this sideways.
            .restrict_scroll_to_axis()
            .font_family(FONT_MONO)
            .text_size(px(crate::ui::theme::FS_MD))
            .line_height(relative(1.65))
            .text_color(t.text)
            .child(rows)
            .into_any_element()
    }

    /// The review notes on the file at `path`.
    fn notes_on(&self, path: &str, cx: &App) -> Vec<ReviewNote> {
        cx.try_global::<Review>()
            .map(|r| {
                r.notes(&self.project_id)
                    .iter()
                    .filter(|n| n.path == path)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// One item of the diff's list: see `diff_view`.
    fn diff_item(
        &mut self,
        path: &str,
        item: usize,
        last: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *theme(cx);
        let Some(parsed) = self.diff_rows.clone() else {
            return div().into_any_element();
        };
        let rows = &parsed.rows;
        let notes = self.notes_on(path, cx);
        let mut children: Vec<AnyElement> = Vec::new();
        let Some(index) = item.checked_sub(1) else {
            // Notes whose lines are no longer in the diff show above it.
            for note in notes
                .iter()
                .filter(|n| !rows.iter().any(|r| anchored_at(n, r)))
            {
                children.push(self.note_card(note, &t, window, cx));
            }
            return div()
                .pt(px(12.))
                .flex()
                .flex_col()
                .children(children)
                .into_any_element();
        };
        let Some(row) = rows.get(index) else {
            return div().into_any_element();
        };
        if row.kind == Kind::Hunk {
            let busy = self.busy;
            children.push(
                self.hunk_row(row, index, rows, &parsed.hunks, busy, &t, cx)
                    .into_any_element(),
            );
        } else {
            let (bg, mark) = match row.kind {
                Kind::Add => (Some(t.diff_add_bg), "+"),
                Kind::Del => (Some(t.diff_del_bg), "−"),
                _ => (None, " "),
            };
            let number = |n: Option<u32>| {
                div()
                    .flex_none()
                    .min_w(px(48.))
                    .pr(px(8.))
                    .flex()
                    .justify_end()
                    .text_color(t.text_subtle)
                    .child(n.map(|n| n.to_string()).unwrap_or_default())
            };
            children.push(
                div()
                    .flex()
                    .w_full()
                    .when_some(bg, gpui::Styled::bg)
                    .child(number(row.old_no))
                    .child(number(row.new_no))
                    .child(
                        div()
                            .flex_none()
                            .w(px(20.))
                            .flex()
                            .justify_center()
                            .text_color(t.text_subtle)
                            .child(mark),
                    )
                    .child(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .child(row.text.clone()),
                    )
                    .into_any_element(),
            );
        }
        for note in notes.iter().filter(|n| anchored_at(n, row)) {
            children.push(self.note_card(note, &t, window, cx));
        }
        if self.draft.as_ref().is_some_and(|d| d.row == index) {
            let place = self
                .draft
                .as_ref()
                .map(|d| d.place.clone())
                .unwrap_or_default();
            children.push(self.note_editor(&place, false, &t, window, cx));
        }
        div()
            .flex()
            .flex_col()
            .when(index + 1 == last, |d| d.pb(px(12.)))
            .children(children)
            .into_any_element()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "each argument is a separate piece of what it draws"
    )]
    fn hunk_row(
        &self,
        row: &Row,
        index: usize,
        rows: &[Row],
        hunks: &[String],
        busy: bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let n = row.hunk + 1;
        let hunk = hunks.get(row.hunk).cloned().unwrap_or_default();
        let staged = self.staged.get(row.hunk).copied().unwrap_or(false);
        let mut end = index;
        while rows.get(end + 1).is_some_and(|r| r.hunk == row.hunk) {
            end += 1;
        }
        let draft = note_for(rows, index + 1, end);
        let hunk_button = |id: String, label: String, text: &'static str| {
            let (hover, fg) = (t.bg_hover, t.text);
            button(SharedString::from(id), label.clone())
                .rounded(px(RADIUS_SM))
                .px(px(6.))
                .py(px(2.))
                .text_color(t.text_muted)
                .tooltip(tooltip(label))
                .when(busy && text != "Note", |b| b.opacity(0.5).cursor_default())
                .when(!busy || text == "Note", |b| {
                    b.hover(move |s| s.bg(hover).text_color(fg))
                })
                .child(text)
        };
        let act = |action: HunkAction| {
            let hunk = hunk.clone();
            cx.listener(move |this: &mut Canvas, _: &gpui::ClickEvent, window, cx| {
                if !this.busy {
                    this.act_on_hunk(hunk.clone(), action, window, cx);
                }
            })
        };
        let (stage_label, stage_text, stage_action) = if staged {
            (
                format!("Unstage change {n}"),
                "Unstage",
                HunkAction::Unstage,
            )
        } else {
            (format!("Stage change {n}"), "Stage", HunkAction::Stage)
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .bg(t.bg_panel)
            .py(px(4.))
            .pl(px(16.))
            .text_color(t.text_subtle)
            .child(div().flex_1().whitespace_nowrap().child(row.text.clone()))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(4.))
                    .pr(px(12.))
                    .pl(px(8.))
                    .font_family(crate::ui::theme::FONT_UI)
                    .text_size(px(FS_XS))
                    .child(
                        hunk_button(
                            format!("note-{n}"),
                            format!("Add a note on change {n}"),
                            "Note",
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if let Some(draft) = draft.clone() {
                                    this.start_draft(draft, window, cx);
                                }
                            },
                        )),
                    )
                    .child(
                        hunk_button(
                            format!("discard-{n}"),
                            format!("Discard change {n}"),
                            "Discard",
                        )
                        .on_click(act(HunkAction::Discard)),
                    )
                    .child(
                        hunk_button(format!("stage-{n}"), stage_label, stage_text)
                            .on_click(act(stage_action)),
                    ),
            )
    }

    fn start_draft(&mut self, draft: Draft, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.draft = Some(draft);
        self.note_input.update(cx, |n, cx| n.set_text("", cx));
        window.focus(&self.note_input.focus_handle(cx), cx);
        cx.notify();
    }

    pub(in crate::canvas) fn cancel_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft = None;
        self.editing = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Saves the note being written or edited, when it has text.
    pub(in crate::canvas) fn save_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.note_input.read(cx).content.trim().to_string();
        if text.is_empty() {
            return;
        }
        let project = self.project_id.clone();
        if let Some(id) = self.editing.take() {
            let review = cx.default_global::<Review>();
            if let Some(note) = review
                .notes
                .get_mut(&project)
                .and_then(|list| list.iter_mut().find(|n| n.id == id))
            {
                note.text = text;
            }
        } else if let (Some(draft), Some(path)) = (self.draft.take(), self.path.clone()) {
            let review = cx.default_global::<Review>();
            review.next += 1;
            let id = review.next;
            review.notes.entry(project).or_default().push(ReviewNote {
                id,
                path,
                place: draft.place,
                anchor: draft.anchor,
                excerpt: draft.excerpt,
                text,
            });
        }
        self.cancel_note(window, cx);
    }

    fn note_editor(
        &self,
        place: &str,
        editing: bool,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = format!("Note on {place}");
        let has_text = !self.note_input.read(cx).content.trim().is_empty();
        let focused = self.note_input.focus_handle(cx).is_focused(window);
        self.note_input
            .update(cx, |n, _| n.placeholder = label.clone().into());
        let save = if editing { "Save" } else { "Add note" };
        div()
            .id("note-editor")
            .role(Role::Group)
            .aria_label(label)
            .my(px(4.))
            .mr(px(16.))
            .ml(px(116.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .p(px(8.))
            .font_family(crate::ui::theme::FONT_UI)
            .text_size(px(FS_SM))
            .line_height(relative(1.5))
            .child(
                div()
                    .min_h(px(60.))
                    .rounded(px(RADIUS_SM))
                    .border_1()
                    .border_color(if focused { t.accent } else { t.border })
                    .bg(t.bg_input)
                    .px(px(8.))
                    .py(px(4.))
                    .child(self.note_input.clone()),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        btn("note-cancel", "Cancel", BtnKind::Plain, false, t)
                            .child("Cancel")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.cancel_note(window, cx)),
                            ),
                    )
                    .child(
                        btn("note-save", save, BtnKind::Primary, !has_text, t)
                            .child(save)
                            .when(has_text, |b| {
                                b.on_click(
                                    cx.listener(|this, _, window, cx| this.save_note(window, cx)),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }

    fn note_card(
        &self,
        note: &ReviewNote,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.editing == Some(note.id) {
            return self.note_editor(&note.place, true, t, window, cx);
        }
        let group = SharedString::from(format!("note-{}", note.id));
        let (hover, fg) = (t.bg_hover, t.text);
        let small = |id: String, label: String, name: &'static str| {
            button(SharedString::from(id), label)
                .size(px(20.))
                .flex_none()
                .justify_center()
                .rounded(px(RADIUS_SM))
                .text_color(t.text_muted)
                .hover(move |s| s.bg(hover).text_color(fg))
                .invisible()
                .group_hover(group.clone(), gpui::Styled::visible)
                .child(crate::ui::icon(name, 13.))
        };
        let (id, text) = (note.id, note.text.clone());
        let project = self.project_id.clone();
        div()
            .id(SharedString::from(format!("note-card-{}", note.id)))
            .group(group.clone())
            .role(Role::Note)
            .aria_label(format!("Note on {}", note.place))
            .my(px(4.))
            .mr(px(16.))
            .ml(px(116.))
            .flex()
            .items_start()
            .gap(px(8.))
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .px(px(12.))
            .py(px(8.))
            .font_family(crate::ui::theme::FONT_UI)
            .text_size(px(FS_SM))
            .line_height(relative(1.5))
            .child(icon("chat-text", 14.).mt(px(2.)).text_color(t.accent_text))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_weight(FontWeight::NORMAL)
                    .child(note.text.clone()),
            )
            .child(
                small(
                    format!("edit-note-{id}"),
                    format!("Edit note on {}", note.place),
                    "pencil-simple",
                )
                .tooltip(tooltip("Edit"))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.draft = None;
                    this.editing = Some(id);
                    let text = text.clone();
                    this.note_input.update(cx, |n, cx| n.set_text(&text, cx));
                    window.focus(&this.note_input.focus_handle(cx), cx);
                    cx.notify();
                })),
            )
            .child(
                small(
                    format!("delete-note-{id}"),
                    format!("Delete note on {}", note.place),
                    "trash",
                )
                .tooltip(tooltip("Delete"))
                .on_click(cx.listener(move |_, _, _, cx| {
                    if let Some(list) = cx.default_global::<Review>().notes.get_mut(&project) {
                        list.retain(|n| n.id != id);
                    }
                    cx.notify();
                })),
            )
            .into_any_element()
    }
}
