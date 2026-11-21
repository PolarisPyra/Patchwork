//! A branch against the commit it left another branch at: every file it changed since, committed
//! or not, and one file's diff over that span.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashMap;

use crate::workspace::{DiffSide, FileChange};

use super::changes::{git_changes, git_file_diff, parse_numstat};
use super::run::{git, is_untracked};

/// The files under `root`, named from it, that differ from commit `base`: what commits made since
/// and what is changed and not committed yet, untracked files included.
///
/// # Errors
///
/// Returns an error when `git diff` or `git status` fails, as for a commit the repository lacks.
pub fn changes_since(root: &str, base: &str) -> Result<Vec<FileChange>, Error> {
    let diff = |what: &str| {
        git(
            root,
            &["diff", "-z", "-M", "--relative", what, base, "--", "."],
        )
    };
    let counts: HashMap<String, (u32, u32)> = parse_numstat(&diff("--numstat")?)
        .into_iter()
        .map(|(path, added, deleted)| (path, (added, deleted)))
        .collect();
    let mut files = name_status(&diff("--name-status")?, &counts);
    files.extend(
        git_changes(root)?
            .into_iter()
            .filter(|c| c.status == "untracked"),
    );
    Ok(files)
}

/// The diff of `rel` from commit `base` to the file as it is now. An untracked file shows all of
/// it.
///
/// # Errors
///
/// Returns an error when `git` fails to run or to diff the file.
pub fn diff_since(root: &str, rel: &str, base: &str) -> Result<String, Error> {
    if is_untracked(root, rel)? {
        return git_file_diff(root, rel, DiffSide::All);
    }
    git(root, &["diff", "-M", base, "--", rel])
}

/// Reads `git diff --name-status -z` into changes, with their line counts from `counts`.
pub(super) fn name_status(names: &str, counts: &HashMap<String, (u32, u32)>) -> Vec<FileChange> {
    let mut files = Vec::new();
    let mut fields = names.split('\0');
    while let Some(code) = fields.next().filter(|c| !c.is_empty()) {
        let renamed = code.starts_with(['R', 'C']);
        let from = renamed.then(|| fields.next()).flatten();
        let Some(path) = fields.next() else { break };
        let (additions, deletions) = counts.get(path).copied().unwrap_or((0, 0));
        files.push(FileChange {
            path: path.to_string(),
            status: match code.chars().next() {
                Some('A' | 'C') => "added",
                Some('D') => "deleted",
                Some('R') => "renamed",
                _ => "modified",
            }
            .into(),
            additions,
            deletions,
            renamed_from: from.filter(|_| code.starts_with('R')).map(String::from),
            ..FileChange::default()
        });
    }
    files
}
