//! The conflicts git left in a file, and settling one by taking a side of it.

// Rust guideline compliant 2026-10-09

use std::io::Read;
use std::path::PathBuf;

use patchwork_platform::{Error, files::paths, ssh};

use crate::model::{Conflict, Take};

/// Maximum file size read when resolving a conflict locally (2 MB).
const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;

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

/// Joins `rel` onto the local project `root` and rejects anything that resolves outside it.
fn resolve_local_inside(root: &str, rel: &str) -> Result<PathBuf, Error> {
    let root = paths::canonicalize(root).map_err(|e| e.to_string())?;
    let path = paths::canonicalize(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
    if path.starts_with(&root) {
        Ok(path)
    } else {
        Err(Error::OutsideProject)
    }
}

/// Reads a local text file inside `root` up to `MAX_READ_BYTES`.
fn read_local_file(root: &str, rel: &str) -> Result<String, Error> {
    let path = resolve_local_inside(root, rel)?;
    if !path.is_file() {
        return Err(format!("{rel} is not a file").into());
    }
    let file = std::fs::File::open(&path).map_err(|e| format!("Cannot read {rel}: {e}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_READ_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Cannot read {rel}: {e}"))?;
    if bytes.len() as u64 > MAX_READ_BYTES {
        return Err("File is larger than 2 MB".into());
    }
    if bytes.contains(&0) {
        return Err("Binary file".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
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
    let text = read_local_file(root, rel)?;
    let settled = settled(&text, index, take)
        .ok_or_else(|| Error::Message("The file changed. Try again.".into()))?;
    std::fs::write(resolve_local_inside(root, rel)?, &settled)
        .map_err(|e| format!("Cannot write {rel}: {e}"))?;
    Ok(conflicts_in(&settled).len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_settles_standard_conflict() {
        let input = "top\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> topic\nbottom\n";
        let found = conflicts_in(input);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].current, "HEAD");
        assert_eq!(found[0].incoming, "topic");
        assert_eq!(
            settled(input, 0, Take::Current).as_deref(),
            Some("top\nours\nbottom\n")
        );
        assert_eq!(
            settled(input, 0, Take::Incoming).as_deref(),
            Some("top\ntheirs\nbottom\n")
        );
        assert_eq!(
            settled(input, 0, Take::Both).as_deref(),
            Some("top\nours\ntheirs\nbottom\n")
        );
    }
}
