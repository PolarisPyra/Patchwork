//! What changed between two versions of an artifact, line by line, with the unchanged lines far
//! from any change folded away.

// Rust guideline compliant 2026-10-09

/// The unchanged lines kept on each side of a change.
const AROUND: usize = 3;

/// The most pairs of lines compared one by one. Past it, the changed middle shows as all of the
/// old lines taken out and all of the new put in, rather than taking long or much memory.
const MOST_PAIRS: usize = 4_000_000;

/// One row of the changes between two versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeRow {
    /// A line both versions have.
    Same(String),
    /// A line only the newer version has.
    Added(String),
    /// A line only the older version has.
    Removed(String),
    /// This many unchanged lines, folded away.
    Folded(usize),
}

/// The rows that turn `old` into `new`, with the unchanged lines far from any change folded.
#[must_use]
pub fn artifact_changes(old: &str, new: &str) -> Vec<ChangeRow> {
    fold(&line_changes(old, new))
}

/// Every line of `old` and `new`, each marked as both have it, or only one of them.
fn line_changes(old: &str, new: &str) -> Vec<ChangeRow> {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let start = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let end = a[start..]
        .iter()
        .rev()
        .zip(b[start..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (mid_a, mid_b) = (&a[start..a.len() - end], &b[start..b.len() - end]);
    let mut rows: Vec<ChangeRow> = a[..start]
        .iter()
        .map(|l| ChangeRow::Same((*l).to_owned()))
        .collect();
    if mid_a.len().saturating_mul(mid_b.len()) > MOST_PAIRS {
        rows.extend(mid_a.iter().map(|l| ChangeRow::Removed((*l).to_owned())));
        rows.extend(mid_b.iter().map(|l| ChangeRow::Added((*l).to_owned())));
    } else {
        rows.extend(common(mid_a, mid_b));
    }
    rows.extend(
        a[a.len() - end..]
            .iter()
            .map(|l| ChangeRow::Same((*l).to_owned())),
    );
    rows
}

/// The rows from `a` to `b` through their longest run of lines in common.
fn common(a: &[&str], b: &[&str]) -> Vec<ChangeRow> {
    let width = b.len() + 1;
    // `longest[i * width + j]`: the most lines `a[i..]` and `b[j..]` have in common, in order.
    let mut longest = vec![0_u32; (a.len() + 1) * width];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            longest[i * width + j] = if a[i] == b[j] {
                longest[(i + 1) * width + j + 1] + 1
            } else {
                longest[(i + 1) * width + j].max(longest[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut rows = Vec::with_capacity(a.len() + b.len());
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            rows.push(ChangeRow::Same(a[i].to_owned()));
            (i, j) = (i + 1, j + 1);
        } else if i < a.len()
            && (j == b.len() || longest[(i + 1) * width + j] >= longest[i * width + j + 1])
        {
            rows.push(ChangeRow::Removed(a[i].to_owned()));
            i += 1;
        } else {
            rows.push(ChangeRow::Added(b[j].to_owned()));
            j += 1;
        }
    }
    rows
}

/// `rows` with each run of unchanged lines more than [`AROUND`] from a change folded.
fn fold(rows: &[ChangeRow]) -> Vec<ChangeRow> {
    let changed: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| !matches!(r, ChangeRow::Same(_)))
        .map(|(i, _)| i)
        .collect();
    let near = |i: usize| {
        let at = changed.partition_point(|&c| c < i);
        let after = changed.get(at).is_some_and(|&c| c - i <= AROUND);
        let before = at > 0 && changed.get(at - 1).is_some_and(|&c| i - c <= AROUND);
        after || before
    };
    let mut out = Vec::new();
    let mut folded = 0;
    for (i, row) in rows.iter().enumerate() {
        if matches!(row, ChangeRow::Same(_)) && !near(i) {
            folded += 1;
            continue;
        }
        if folded > 0 {
            out.push(ChangeRow::Folded(folded));
            folded = 0;
        }
        out.push(row.clone());
    }
    if folded > 0 {
        out.push(ChangeRow::Folded(folded));
    }
    out
}
