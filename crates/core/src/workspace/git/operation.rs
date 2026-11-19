//! A rebase, merge, cherry-pick or revert: starting one, and continuing or aborting one that
//! stopped on conflicts. Also taking the last commit back and moving the branch.

// Rust guideline compliant 2026-10-09

use crate::Error;
use crate::workspace::Operation;

use super::run::{explained, git, git_succeeds, has_head};

/// The files git keeps in its folder while each operation is under way.
const MARKS: [(&str, Operation); 5] = [
    ("rebase-merge", Operation::Rebase),
    ("rebase-apply", Operation::Rebase),
    ("MERGE_HEAD", Operation::Merge),
    ("CHERRY_PICK_HEAD", Operation::CherryPick),
    ("REVERT_HEAD", Operation::Revert),
];

/// The files a merge or a squash leaves its message for the next commit in.
const PREPARED: [&str; 2] = ["MERGE_MSG", "SQUASH_MSG"];

/// Git's editor setting that takes a message as it is, since there is no editor to show one in.
const NO_EDITOR: [&str; 2] = ["-c", "core.editor=true"];

/// Where each of `names` is in the repository's git folder, absolute.
fn git_paths(root: &str, names: &[&str]) -> Vec<String> {
    let mut args = vec!["rev-parse", "--path-format=absolute"];
    for name in names {
        args.extend(["--git-path", name]);
    }
    git(root, &args)
        .map(|paths| paths.lines().map(|p| p.trim().to_string()).collect())
        .unwrap_or_default()
}

/// The rebase, merge, cherry-pick or revert that stopped in the repository at `root`, if any.
pub(in crate::workspace) fn operation(root: &str) -> Option<Operation> {
    let names: Vec<&str> = MARKS.iter().map(|(name, _)| *name).collect();
    let paths = git_paths(root, &names);
    let exists: Vec<bool> = match crate::system::ssh::split(root) {
        // The paths are on the host.
        Some(at) => crate::system::ssh::fs::sizes(at.host, "/", &paths)
            .map(|sizes| sizes.iter().map(Option::is_some).collect())
            .unwrap_or_default(),
        None => paths
            .iter()
            .map(|path| std::path::Path::new(path).exists())
            .collect(),
    };
    MARKS
        .iter()
        .zip(exists)
        .find(|(_, there)| *there)
        .map(|((_, operation), _)| *operation)
}

/// The message git prepared for the next commit, without its comment lines. None over SSH, where
/// the file is on the host.
pub(in crate::workspace) fn prepared_message(root: &str) -> Option<String> {
    if crate::system::ssh::split(root).is_some() {
        return None;
    }
    git_paths(root, &PREPARED)
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .map(|text| {
            text.lines()
                .filter(|line| !line.starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string()
        })
        .find(|message| !message.is_empty())
}

/// The error for `failed` when it left `what` stopped on conflicts, to resolve and continue;
/// else git's own error, in plain words where there are some.
pub(in crate::workspace) fn stopped(root: &str, what: &str, failed: Error) -> Error {
    match operation(root) {
        Some(operation) => Error::Tool {
            message: format!(
                "{what} stopped on conflicts. Resolve them and stage the files, then continue \
                 the {}.",
                operation.name()
            ),
            detail: failed.to_string(),
        },
        None => explained(failed),
    }
}

/// Refuses to start something while a rebase, merge, cherry-pick or revert is stopped.
pub(in crate::workspace) fn none_stopped(root: &str) -> Result<(), Error> {
    match operation(root) {
        Some(operation) => Err(Error::Message(format!(
            "Continue or abort the {} in progress first",
            operation.name()
        ))),
        None => Ok(()),
    }
}

/// Runs `git <args>`, which may stop on conflicts, taking any message as git writes it.
fn start(root: &str, what: &str, args: &[&str]) -> Result<(), Error> {
    none_stopped(root)?;
    let mut all = NO_EDITOR.to_vec();
    all.extend_from_slice(args);
    git(root, &all)
        .map(|_| ())
        .map_err(|e| stopped(root, what, e))
}

/// Merges the branch, tag or commit `name` into the checked-out branch.
///
/// # Errors
///
/// Returns an error when another operation is stopped, the merge stops on conflicts, or `git
/// merge` fails.
pub fn merge(root: &str, name: &str) -> Result<(), Error> {
    start(root, "The merge", &["merge", "--no-edit", name])
}

/// Replays the checked-out branch's own commits on top of `onto`.
///
/// # Errors
///
/// Returns an error when another operation is stopped, the rebase stops on conflicts, or `git
/// rebase` fails.
pub fn rebase(root: &str, onto: &str) -> Result<(), Error> {
    start(root, "The rebase", &["rebase", "--autostash", onto])
}

/// Applies what commit `hash` changed as a new commit on the checked-out branch.
///
/// # Errors
///
/// Returns an error when another operation is stopped, the cherry-pick stops on conflicts, or
/// `git cherry-pick` fails, as for a merge commit.
pub fn cherry_pick(root: &str, hash: &str) -> Result<(), Error> {
    start(root, "The cherry-pick", &["cherry-pick", hash])
}

/// Adds a commit that takes back what commit `hash` changed.
///
/// # Errors
///
/// Returns an error when another operation is stopped, the revert stops on conflicts, or `git
/// revert` fails, as for a merge commit.
pub fn revert_commit(root: &str, hash: &str) -> Result<(), Error> {
    start(root, "The revert", &["revert", "--no-edit", hash])
}

/// Goes on with the stopped operation, and returns which it was.
///
/// # Errors
///
/// Returns an error when nothing is stopped, a file is still in conflict, or git stops again.
pub fn continue_operation(root: &str) -> Result<Operation, Error> {
    let operation =
        operation(root).ok_or_else(|| Error::Message("Nothing is waiting to continue".into()))?;
    if !git(root, &["diff", "--name-only", "--diff-filter=U"])?
        .trim()
        .is_empty()
    {
        return Err(Error::Message(
            "Resolve the conflicts and stage the files first".into(),
        ));
    }
    let verb = match operation {
        Operation::Rebase => "rebase",
        Operation::Merge => "merge",
        Operation::CherryPick => "cherry-pick",
        Operation::Revert => "revert",
    };
    let mut args = NO_EDITOR.to_vec();
    args.extend([verb, "--continue"]);
    git(root, &args).map_err(|e| stopped(root, &format!("The {}", operation.name()), e))?;
    Ok(operation)
}

/// Gives up the stopped operation, putting the branch back as it was before it, and returns
/// which it was.
///
/// # Errors
///
/// Returns an error when nothing is stopped or git cannot abort it.
pub fn abort_operation(root: &str) -> Result<Operation, Error> {
    let operation =
        operation(root).ok_or_else(|| Error::Message("Nothing is waiting to abort".into()))?;
    let verb = match operation {
        Operation::Rebase => "rebase",
        Operation::Merge => "merge",
        Operation::CherryPick => "cherry-pick",
        Operation::Revert => "revert",
    };
    git(root, &[verb, "--abort"]).map_err(explained)?;
    Ok(operation)
}

/// Takes the last commit back, leaving what it changed staged, and returns its message.
///
/// # Errors
///
/// Returns an error when there is no commit, an operation is stopped, or `git` fails.
pub fn undo_commit(root: &str) -> Result<String, Error> {
    if !has_head(root) {
        return Err(Error::Message("There is no commit to undo".into()));
    }
    none_stopped(root)?;
    let message = git(root, &["log", "-1", "--format=%B"])?.trim().to_string();
    if git_succeeds(root, &["rev-parse", "--verify", "-q", "HEAD~1"]) {
        git(root, &["reset", "--soft", "HEAD~1"])?;
    } else {
        // The first commit has none before it to go back to: the branch is unborn again.
        git(root, &["update-ref", "-d", "HEAD"])?;
    }
    Ok(message)
}

/// Moves the checked-out branch to commit `hash`. The commits after it leave the branch: their
/// changes stay staged, or with `hard` are thrown away with every uncommitted change.
///
/// # Errors
///
/// Returns an error when an operation is stopped or `git reset` fails.
pub fn reset_to(root: &str, hash: &str, hard: bool) -> Result<(), Error> {
    none_stopped(root)?;
    let mode = if hard { "--hard" } else { "--soft" };
    git(root, &["reset", mode, hash, "--"])
        .map(|_| ())
        .map_err(explained)
}
