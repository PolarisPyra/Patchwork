//! One file's diff (`components/DiffView.tsx`), each change with its Note, Discard and Stage
//! buttons, and the review notes for the agent under the lines they are about
//! (`components/ReviewNotes.tsx` and `review.ts`).

use std::collections::HashMap;

use gpui::{
    AnyElement, Context, Div, Focusable, FontWeight, Global, Role, SharedString, Window, div,
    prelude::*, px, relative,
};
use patchwork_server::workspace::HunkAction;

use super::{Canvas};
use crate::ui::theme::{FONT_MONO, FS_SM, FS_XS, RADIUS, RADIUS_SM, Theme};
use crate::ui::{BtnKind, btn, button, icon, tooltip};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Hunk,
    Add,
    Del,
    Ctx,
}

struct Row {
    kind: Kind,
    text: String,
    /// The change it belongs to, by its place in the diff.
    hunk: usize,
    old_no: Option<u32>,
    new_no: Option<u32>,
}

/// Parses a single-file unified diff into its rows, and each change's text from its `@@` line on,
/// as git takes it back. Header lines are dropped.
fn parse_diff(diff: &str) -> (Vec<Row>, Vec<String>) {
    let mut rows = Vec::new();
    let mut hunks: Vec<String> = Vec::new();
    // The lines of the change being read; None in a file's header.
    let mut lines: Option<Vec<&str>> = None;
    let (mut old_no, mut new_no) = (0u32, 0u32);
    let end = |lines: &mut Option<Vec<&str>>, hunks: &mut Vec<String>| {
        if let Some(l) = lines.take() {
            hunks.push(l.join("\n"));
        }
    };
    for line in diff.split('\n') {
        if let Some((old, new)) = hunk_header(line) {
            end(&mut lines, &mut hunks);
            (old_no, new_no) = (old, new);
            lines = Some(vec![line]);
            rows.push(Row {
                kind: Kind::Hunk,
                text: line.to_string(),
                hunk: hunks.len(),
                old_no: None,
                new_no: None,
            });
            continue;
        }
        // Another file's part, as in a repository without commits, starts with a header of its own.
        if line.starts_with("diff --git ") {
            end(&mut lines, &mut hunks);
        }
        let Some(current) = lines.as_mut() else {
            continue;
        };
        let hunk = hunks.len();
        let row = |kind, text: &str, old_no, new_no| Row {
            kind,
            text: text.replace('\t', "  "),
            hunk,
            old_no,
            new_no,
        };
        if let Some(text) = line.strip_prefix('+') {
            rows.push(row(Kind::Add, text, None, Some(new_no)));
            new_no += 1;
        } else if let Some(text) = line.strip_prefix('-') {
            rows.push(row(Kind::Del, text, Some(old_no), None));
            old_no += 1;
        } else if let Some(text) = line.strip_prefix(' ') {
            rows.push(row(Kind::Ctx, text, Some(old_no), Some(new_no)));
            old_no += 1;
            new_no += 1;
        } else if !line.starts_with('\\') {
            // Trailing blank lines fall through; "\ No newline at end of file" stays in the change.
            continue;
        }
        current.push(line);
    }
    end(&mut lines, &mut hunks);
    (rows, hunks)
}

/// `@@ -old[,n] +new[,n] @@`'s two starting lines.
fn hunk_header(line: &str) -> Option<(u32, u32)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(' ')?;
    let new = rest.strip_prefix('+')?.split_once(' ')?;
    if !new.1.starts_with("@@") {
        return None;
    }
    let start = |s: &str| s.split(',').next()?.parse().ok();
    Some((start(old)?, start(new.0)?))
}

/// Where a note's lines are: the line it shows under, by its number in the new file, or in the old
/// one for removed lines.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Anchor {
    old: bool,
    line: u32,
}

#[derive(Clone)]
pub struct ReviewNote {
    id: usize,
    path: String,
    /// "lines 12–14", or "removed line 8".
    place: String,
    anchor: Anchor,
    /// The diff lines it is about, each with its +, - or space.
    excerpt: Vec<String>,
    text: String,
}

/// Notes on lines of the diffs, kept by project until they go to the agent as one review.
#[derive(Default)]
pub struct Review {
    notes: HashMap<String, Vec<ReviewNote>>,
    next: usize,
}

impl Global for Review {}

impl Review {
    pub fn notes(&self, project: &str) -> &[ReviewNote] {
        self.notes
            .get(project)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn clear(&mut self, project: &str) {
        self.notes.remove(project);
    }
}

/// Lines of a note's excerpt the review quotes; the rest are cut.
const EXCERPT_LINES: usize = 20;

/// The notes as one message to the agent: each with its file, lines, and the lines it is about.
pub fn review_message(list: &[ReviewNote]) -> String {
    let parts: Vec<String> = list
        .iter()
        .enumerate()
        .map(|(i, note)| {
            let mut lines: Vec<&str> = note.excerpt.iter().map(String::as_str).collect();
            if lines.len() > EXCERPT_LINES {
                lines.truncate(EXCERPT_LINES);
                lines.push("…");
            }
            // A fence longer than any run of backticks in the lines, so none of them ends it.
            let longest = lines
                .iter()
                .flat_map(|l| l.split(|c| c != '`').map(str::len))
                .max()
                .unwrap_or(0)
                .max(2);
            let fence = "`".repeat(longest + 1);
            format!(
                "{}. {}, {}:\n{fence}diff\n{}\n{fence}\n{}",
                i + 1,
                note.path,
                note.place,
                lines.join("\n"),
                note.text.trim()
            )
        })
        .collect();
    format!(
        "Review notes on your changes. Address each one:\n\n{}",
        parts.join("\n\n")
    )
}

/// A note being written: where it goes and what it is about.
#[derive(Clone)]
pub struct Draft {
    /// The row it opens under.
    row: usize,
    place: String,
    anchor: Anchor,
    excerpt: Vec<String>,
}

/// A note's place and lines, from the rows `from` to `to`; None when they are only headers.
fn note_for(rows: &[Row], from: usize, to: usize) -> Option<Draft> {
    let picked: Vec<&Row> = rows
        .get(from..=to.min(rows.len().saturating_sub(1)))?
        .iter()
        .filter(|r| r.kind != Kind::Hunk)
        .collect();
    let last = picked.last()?;
    let span = |nos: &[u32]| match (nos.first(), nos.last()) {
        (Some(a), Some(b)) if a == b => format!("line {a}"),
        (Some(a), Some(b)) => format!("lines {a}–{b}"),
        _ => String::new(),
    };
    let added: Vec<u32> = picked
        .iter()
        .filter(|r| r.kind != Kind::Del)
        .map(|r| r.new_no.unwrap_or(0))
        .collect();
    let removed: Vec<u32> = picked
        .iter()
        .filter_map(|r| r.old_no)
        .filter(|&n| n > 0)
        .collect();
    let mut row = to;
    while row > from && rows[row].kind == Kind::Hunk {
        row -= 1;
    }
    Some(Draft {
        row,
        place: if added.is_empty() {
            format!("removed {}", span(&removed))
        } else {
            span(&added)
        },
        anchor: if last.kind == Kind::Del {
            Anchor {
                old: true,
                line: last.old_no.unwrap_or(0),
            }
        } else {
            Anchor {
                old: false,
                line: last.new_no.unwrap_or(0),
            }
        },
        excerpt: picked
            .iter()
            .map(|r| {
                let mark = match r.kind {
                    Kind::Add => '+',
                    Kind::Del => '-',
                    _ => ' ',
                };
                format!("{mark}{}", r.text)
            })
            .collect(),
    })
}

/// Whether `note` shows under `row`.
fn anchored_at(note: &ReviewNote, row: &Row) -> bool {
    if note.anchor.old {
        row.kind == Kind::Del && row.old_no == Some(note.anchor.line)
    } else {
        row.kind != Kind::Del && row.kind != Kind::Hunk && row.new_no == Some(note.anchor.line)
    }
}

/// A monospace character's width at `--fs-md`, for the widest line's floor.
const CHAR_W: f32 = 7.8;

impl Canvas {
    pub(super) fn diff_view(
        &mut self,
        diff: &str,
        path: &str,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (rows, hunks) = parse_diff(diff);
        self.hunk_children.clear();
        if rows.is_empty() {
            return super::body::centered(div())
                .text_color(t.text_muted)
                .child("No changes in this file.")
                .into_any_element();
        }
        let notes: Vec<ReviewNote> = cx
            .try_global::<Review>()
            .map(|r| {
                r.notes(&self.project_id)
                    .iter()
                    .filter(|n| n.path == path)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        // Rows have two 48px number columns and a 20px marker; hunk headers 16px padding a side.
        let widest = rows
            .iter()
            .map(|r| {
                let chars = r.text.chars().count() as f32 * CHAR_W;
                if r.kind == Kind::Hunk {
                    chars + 32. + 200.
                } else {
                    chars + 116.
                }
            })
            .fold(0., f32::max);
        let mut children: Vec<AnyElement> = Vec::new();
        // Notes whose lines are no longer in the diff show above it.
        for note in notes
            .iter()
            .filter(|n| !rows.iter().any(|r| anchored_at(n, r)))
        {
            children.push(self.note_card(note, t, window, cx));
        }
        let busy = self.busy;
        for (index, row) in rows.iter().enumerate() {
            if row.kind == Kind::Hunk {
                self.hunk_children.push(children.len());
                children.push(
                    self.hunk_row(row, index, &rows, &hunks, busy, t, cx)
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
                        .min_w(px(widest))
                        .when_some(bg, |d, bg| d.bg(bg))
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
                children.push(self.note_card(note, t, window, cx));
            }
            if self.draft.as_ref().is_some_and(|d| d.row == index) {
                let place = self
                    .draft
                    .as_ref()
                    .map(|d| d.place.clone())
                    .unwrap_or_default();
                children.push(self.note_editor(&place, false, t, window, cx));
            }
        }
        div()
            .id("diff")
            .flex()
            .flex_col()
            .py(px(12.))
            .min_w_full()
            .font_family(FONT_MONO)
            .text_size(px(crate::ui::theme::FS_MD))
            .line_height(relative(1.65))
            .text_color(t.text)
            .children(children)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
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
                    this.act_on_hunk(hunk.clone(), action, window, cx)
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

    pub(super) fn cancel_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft = None;
        self.editing = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Saves the note being written or edited, when it has text.
    pub(super) fn save_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                .group_hover(group.clone(), |s| s.visible())
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
