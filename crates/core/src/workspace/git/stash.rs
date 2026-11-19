//! The stash: setting changes aside, and bringing them back.

// Rust guideline compliant 2026-10-09

use crate::Error;
use crate::workspace::GitStash;

use super::run::{explained, git, git_raw};

/// The stash's entries, newest first.
///
/// # Errors
///
/// Returns an error when `git stash list` fails.
pub fn git_stashes(root: &str) -> Result<Vec<GitStash>, Error> {
    let out = git(root, &["stash", "list", "--format=%gd%x1f%gs"])?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let (name, message) = line.split_once('\x1f')?;
            let index = name.strip_prefix("stash@{")?.strip_suffix('}')?;
            Some(GitStash {
                index: index.parse().ok()?,
                message: message.to_string(),
            })
        })
        .collect())
}

/// Sets the uncommitted changes aside as a stash entry named `message`, and puts the files back
/// as they were at the last commit. `untracked` takes new files too, and `staged_only` takes
/// only what is staged.
///
/// # Errors
///
/// Returns an error when there is nothing to stash or `git stash push` fails.
pub fn stash(root: &str, message: &str, untracked: bool, staged_only: bool) -> Result<(), Error> {
    let mut args = vec!["stash", "push"];
    if untracked {
        args.push("--include-untracked");
    }
    if staged_only {
        args.push("--staged");
    }
    if !message.trim().is_empty() {
        args.extend(["-m", message]);
    }
    let out = git_raw(root, &args)?;
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // Git says there was nothing to save and still succeeds.
    if !out.status.success() || said.contains("No local changes to save") {
        return Err(explained(Error::Git(format!(
            "git stash push: {}",
            said.trim()
        ))));
    }
    Ok(())
}

/// Brings the stash entry at `index` back into the files. With `drop` the entry leaves the stash
/// once it applied without conflicts.
///
/// # Errors
///
/// Returns an error when `git stash` fails or the entry conflicts with the files as they are,
/// which keeps the entry.
pub fn stash_apply(root: &str, index: usize, drop: bool) -> Result<(), Error> {
    let entry = format!("stash@{{{index}}}");
    let verb = if drop { "pop" } else { "apply" };
    git(root, &["stash", verb, "--index", &entry])
        .or_else(|_| git(root, &["stash", verb, &entry]))
        .map(|_| ())
        .map_err(|e| {
            if e.to_string().contains("CONFLICT") {
                Error::Tool {
                    message: "The stash conflicts with the files as they are now. Resolve the \
                              conflicts; the stash entry was kept."
                        .into(),
                    detail: e.to_string(),
                }
            } else {
                explained(e)
            }
        })
}

/// Deletes the stash entry at `index`.
///
/// # Errors
///
/// Returns an error when `git stash drop` fails, as for an entry that is gone.
pub fn stash_drop(root: &str, index: usize) -> Result<(), Error> {
    git(root, &["stash", "drop", &format!("stash@{{{index}}}")])
        .map(|_| ())
        .map_err(explained)
}
