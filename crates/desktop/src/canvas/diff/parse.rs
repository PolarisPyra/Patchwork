//! A diff read into rows.

// Rust guideline compliant 2026-10-09

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::canvas) enum Kind {
    Hunk,
    Add,
    Del,
    Ctx,
}

pub(in crate::canvas) struct Row {
    pub(in crate::canvas) kind: Kind,
    pub(super) text: String,
    /// The change it belongs to, by its place in the diff.
    pub(super) hunk: usize,
    pub(super) old_no: Option<u32>,
    pub(super) new_no: Option<u32>,
}

/// A diff read once, when it loads: its rows, each change's text, and the widest row in pixels.
pub(in crate::canvas) struct Parsed {
    pub(in crate::canvas) rows: Vec<Row>,
    pub(in crate::canvas) hunks: Vec<String>,
    pub(in crate::canvas) widest: f32,
}

/// Reads `diff` into its rows and measures the widest, which the list is laid out to.
pub(in crate::canvas) fn parse(diff: &str) -> Parsed {
    let (rows, hunks) = parse_diff(diff);
    // Rows have two 48px number columns and a 20px marker; hunk headers 16px padding a side.
    let widest = rows
        .iter()
        .map(|r| {
            let chars = r.text.chars().count() as f32 * super::view::CHAR_W;
            if r.kind == Kind::Hunk {
                chars + 32. + 200.
            } else {
                chars + 116.
            }
        })
        .fold(0., f32::max);
    Parsed {
        rows,
        hunks,
        widest,
    }
}

/// Parses a single-file unified diff into its rows, and each change's text from its `@@` line on,
/// as git takes it back. Header lines are dropped.
pub(super) fn parse_diff(diff: &str) -> (Vec<Row>, Vec<String>) {
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
pub(super) fn hunk_header(line: &str) -> Option<(u32, u32)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(' ')?;
    let new = rest.strip_prefix('+')?.split_once(' ')?;
    if !new.1.starts_with("@@") {
        return None;
    }
    let start = |s: &str| s.split(',').next()?.parse().ok();
    Some((start(old)?, start(new.0)?))
}
