//! The marked text as it is copied: plain, and as Markdown.

// Rust guideline compliant 2026-10-09

use std::ops::Range;

use super::state::{Kind, Known, Mark};

/// The marked part of each text, top to bottom, as it reads: a blank line between blocks, list
/// items and table rows on their own lines, and a tab between the cells of a row.
pub(super) fn plain(pieces: &[(&Known, Range<usize>)]) -> String {
    let mut out = String::new();
    let mut before: Option<&Known> = None;
    for (known, range) in pieces {
        let kind = &known.kind;
        if let Some(b) = before {
            out.push_str(match (&b.kind, kind) {
                (Kind::Cell { .. }, Kind::Cell { column, .. }) if *column > 0 => "\t",
                (Kind::Item, Kind::Item) | (Kind::Cell { .. }, Kind::Cell { .. }) => "\n",
                _ => "\n\n",
            });
        }
        if let (Kind::Item, Some(prefix)) = (kind, &known.prefix)
            && range.start == 0
        {
            out.push_str(&prefix.replace("> ", ""));
        }
        out.push_str(&known.layout.text()[range.clone()]);
        before = Some(known);
    }
    out
}

/// The marked part of each text, top to bottom, as Ctrl+C copies it, written back as Markdown so
/// a paste into a message draws it as it was drawn here. Part of one text keeps only its inline
/// marks, and none that wraps all of it, so a bold word or a line of code copies as it reads.
pub(super) fn markdown(pieces: &[(&Known, Range<usize>)]) -> String {
    if let [(known, range)] = pieces {
        let text = known.layout.text();
        if matches!(known.kind, Kind::Code(_)) {
            return text[range.clone()].to_string();
        }
        return marked(&text, range, &known.marks, true);
    }
    let mut out = String::new();
    let mut before: Option<&Known> = None;
    // The cells copied in the open table row, for the line under a header row.
    let mut cells = 0;
    for (known, range) in pieces {
        let text = known.layout.text();
        let (kind, lead) = (&known.kind, known.lead.as_ref());
        let next_cell = matches!(kind, Kind::Cell { column, .. } if *column > 0)
            && matches!(before.map(|b| &b.kind), Some(Kind::Cell { .. }));
        if !next_cell {
            end_row(&mut out, before, cells);
            let line = match &known.prefix {
                Some(prefix) if range.start == 0 => prefix.as_ref(),
                _ => lead,
            };
            match before {
                // The first line keeps no indent, which would read as code.
                None => out.push_str(line.trim_start()),
                Some(b) => {
                    let tight = matches!(
                        (&b.kind, kind),
                        (Kind::Item, Kind::Item) | (Kind::Cell { .. }, Kind::Cell { .. })
                    );
                    if !tight {
                        out.push('\n');
                        out.push_str(shared(&b.lead, lead).trim_end());
                    }
                    out.push('\n');
                    out.push_str(line);
                }
            }
        }
        match kind {
            Kind::Cell { .. } => {
                if next_cell {
                    out.push_str(" | ");
                } else {
                    out.push_str("| ");
                    cells = 0;
                }
                cells += 1;
                let cell = marked(&text, range, &known.marks, false);
                out.push_str(&cell.replace('|', "\\|").replace('\n', " "));
            }
            Kind::Code(lang) => {
                let code = &text[range.clone()];
                let mut fence = "```".to_string();
                while code.contains(fence.as_str()) {
                    fence.push('`');
                }
                out.push_str(&fence);
                out.push_str(lang);
                for line in code.split('\n') {
                    out.push('\n');
                    out.push_str(lead);
                    out.push_str(line);
                }
                out.push('\n');
                out.push_str(lead);
                out.push_str(&fence);
            }
            Kind::Heading(level) => {
                if range.start == 0 {
                    out.push_str(&"#".repeat(*level));
                    out.push(' ');
                }
                out.push_str(&marked(&text, range, &known.marks, false).replace('\n', " "));
            }
            Kind::Paragraph | Kind::Item => {
                // Two spaces keep a line break inside a paragraph, which a bare newline joins up.
                let lines = marked(&text, range, &known.marks, false);
                out.push_str(&lines.replace('\n', &format!("  \n{lead}")));
            }
        }
        before = Some(known);
    }
    end_row(&mut out, before, cells);
    out
}

/// `range` of `text` with each mark's Markdown put back around the part of it inside the range.
/// With `inside`, a mark that wraps the whole range is left out.
pub(super) fn marked(text: &str, range: &Range<usize>, marks: &[Mark], inside: bool) -> String {
    // Where each opener and closer goes: closers before openers at one place, and of several
    // openers the one reaching furthest first, so the marks nest as they did.
    let mut edits: Vec<(usize, bool, usize, usize, &str)> = Vec::new();
    for (i, (span, open, close)) in marks.iter().enumerate() {
        let (mut start, mut end) = (span.start.max(range.start), span.end.min(range.end));
        // `**` beside a space does not open or close, so the marker moves in past it.
        while let Some(c) = text.get(start..end).and_then(|s| s.chars().next())
            && c.is_whitespace()
        {
            start += c.len_utf8();
        }
        while let Some(c) = text.get(start..end).and_then(|s| s.chars().next_back())
            && c.is_whitespace()
        {
            end -= c.len_utf8();
        }
        if start >= end || (inside && start == range.start && end == range.end) {
            continue;
        }
        edits.push((start, true, usize::MAX - end, i, open));
        edits.push((end, false, usize::MAX - start, usize::MAX - i, close));
    }
    edits.sort_by_key(|&(at, opens, reach, order, _)| (at, opens, reach, order));
    let mut out = String::new();
    let mut from = range.start;
    for (at, _, _, _, piece) in edits {
        out.push_str(text.get(from..at).unwrap_or(""));
        out.push_str(piece);
        from = at;
    }
    out.push_str(text.get(from..range.end).unwrap_or(""));
    out
}

/// The start two leads share, such as the `> ` of two paragraphs in one quote.
pub(super) fn shared<'a>(a: &'a str, b: &str) -> &'a str {
    let n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    a.get(..n).unwrap_or("")
}

/// Closes a table row when `before`, the last text copied, is a cell, and puts the line that
/// marks a header under a header row of `cells` cells.
pub(super) fn end_row(out: &mut String, before: Option<&Known>, cells: usize) {
    if let Some(known) = before
        && let Kind::Cell { head, .. } = known.kind
    {
        out.push_str(" |");
        if head {
            out.push('\n');
            out.push_str(&known.lead);
            out.push('|');
            out.push_str(&" --- |".repeat(cells));
        }
    }
}

/// The word around byte `at` of `text`, or the run of blanks or of punctuation it is in, within
/// its line: what a double click takes in the chat, the fields and the terminal alike.
#[must_use]
pub fn word_at(text: &str, at: usize) -> Range<usize> {
    let kind = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            0
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    };
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let (before, after) = text.split_at(at);
    // Between two runs, the press belongs to the one after it, unless it is a line's end.
    let Some(here) = after
        .chars()
        .next()
        .filter(|c| *c != '\n')
        .or_else(|| before.chars().next_back().filter(|c| *c != '\n'))
    else {
        return at..at;
    };
    let here = kind(here);
    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| kind(*c) == here && *c != '\n')
        .last()
        .map_or(before.len(), |(i, _)| i);
    let end = after
        .char_indices()
        .find(|(_, c)| kind(*c) != here || *c == '\n')
        .map_or(text.len(), |(i, _)| before.len() + i);
    start..end
}
