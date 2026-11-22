//! The conflicts git left in a file, and settling one by taking a side of it.

// Rust guideline compliant 2026-10-09

use crate::Error;
use crate::system::ssh;

use crate::projects::{read_file, resolve_inside};
use super::types::{Conflict, Take};

/// The line that opens a conflict, before the name of the side that follows.
const OPENS: &str = "<<<<<<<";

/// The line that opens the common ancestor's lines, in a file merged with `diff3`.
const ANCESTOR: &str = "|||||||";

/// The line between the two sides.
const DIVIDES: &str = "=======";

/// The line that closes a conflict, before the name of the side above it.
const CLOSES: &str = ">>>>>>>";

/// Whether `line` is the marker `mark`, alone or before a side's name.
fn marks(line: &str, mark: &str) -> bool {
    line.strip_prefix(mark)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
}

/// The conflicts in `text`, in order. Markers that do not make a whole conflict are left out.
#[must_use]
pub fn conflicts_in(text: &str) -> Vec<Conflict> {
    let mut found = Vec::new();
    let mut open: Option<(usize, Option<usize>, Option<usize>, String)> = None;
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if marks(line, OPENS) {
            open = Some((i, None, None, line[OPENS.len()..].trim().to_string()));
            continue;
        }
        let Some((opens, ancestor, divides, current)) = &mut open else {
            continue;
        };
        if marks(line, ANCESTOR) && divides.is_none() {
            *ancestor = Some(i);
        } else if line == DIVIDES && divides.is_none() {
            *divides = Some(i);
        } else if marks(line, CLOSES) {
            if let Some(divides) = *divides {
                found.push(Conflict {
                    opens: *opens,
                    ancestor: *ancestor,
                    divides,
                    closes: i,
                    current: std::mem::take(current),
                    incoming: line[CLOSES.len()..].trim().to_string(),
                });
            }
            open = None;
        }
    }
    found
}

/// `text` with its conflict number `index`, counted from zero, settled by keeping `take`. Lines
/// keep the endings they had.
fn settled(text: &str, index: usize, take: Take) -> Option<String> {
    let conflict = conflicts_in(text).into_iter().nth(index)?;
    let own_end = conflict.ancestor.unwrap_or(conflict.divides);
    let own = conflict.opens + 1..own_end;
    let incoming = conflict.divides + 1..conflict.closes;
    Some(
        text.split_inclusive('\n')
            .enumerate()
            .filter(|(i, _)| {
                !(conflict.opens..=conflict.closes).contains(i)
                    || (take != Take::Incoming && own.contains(i))
                    || (take != Take::Current && incoming.contains(i))
            })
            .map(|(_, line)| line)
            .collect(),
    )
}

/// Settles conflict number `index`, counted from zero, of the file `rel` by keeping `take`, and
/// returns how many conflicts the file still has.
///
/// The file stays unstaged: staging it is what tells git the file is resolved.
///
/// # Errors
///
/// Returns an error when the project is on another machine, the file cannot be read or written,
/// or it has no such conflict, as when it changed on disk since it was shown.
pub fn resolve_conflict(root: &str, rel: &str, index: usize, take: Take) -> Result<usize, Error> {
    if ssh::split(root).is_some() {
        return Err("Resolve conflicts in an editor on the host".into());
    }
    let text = read_file(root, rel)?;
    let settled = settled(&text, index, take)
        .ok_or_else(|| Error::Message("The file changed. Try again.".into()))?;
    std::fs::write(resolve_inside(root, rel)?, &settled)
        .map_err(|e| format!("Cannot write {rel}: {e}"))?;
    Ok(conflicts_in(&settled).len())
}
