//! Notes left on a diff's lines, and the message that sends them to the agent.

// Rust guideline compliant 2026-10-09

use super::parse::Kind;
use super::parse::Row;
use std::collections::HashMap;
use gpui::{Global};

/// Where a note's lines are: the line it shows under, by its number in the new file, or in the old
/// one for removed lines.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Anchor {
    old: bool,
    line: u32,
}

#[derive(Clone)]
pub struct ReviewNote {
    pub(super) id: usize,
    pub(super) path: String,
    /// "lines 12–14", or "removed line 8".
    pub(super) place: String,
    pub(super) anchor: Anchor,
    /// The diff lines it is about, each with its +, - or space.
    pub(super) excerpt: Vec<String>,
    pub(super) text: String,
}

/// Notes on lines of the diffs, kept by project until they go to the agent as one review.
#[derive(Default)]
pub struct Review {
    pub(super) notes: HashMap<String, Vec<ReviewNote>>,
    pub(super) next: usize,
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
pub(super) const EXCERPT_LINES: usize = 20;

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
    pub(super) row: usize,
    pub(super) place: String,
    pub(super) anchor: Anchor,
    pub(super) excerpt: Vec<String>,
}

/// A note's place and lines, from the rows `from` to `to`; None when they are only headers.
pub(super) fn note_for(rows: &[Row], from: usize, to: usize) -> Option<Draft> {
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
pub(super) fn anchored_at(note: &ReviewNote, row: &Row) -> bool {
    if note.anchor.old {
        row.kind == Kind::Del && row.old_no == Some(note.anchor.line)
    } else {
        row.kind != Kind::Del && row.kind != Kind::Hunk && row.new_no == Some(note.anchor.line)
    }
}
