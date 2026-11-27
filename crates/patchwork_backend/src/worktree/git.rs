//! The git commands Patchwork runs in a task's worktree, with the repository's hooks and
//! helpers turned off, and what they read: the worktrees a repository lists, and whether a
//! checkout is busy.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::fs::File;
use crate::source_control::{self};
use crate::Error;
use super::state::Worktree;

/// Why a task can't use a worktree its record names outside Patchwork's worktrees folders,
/// such as one a development build read from the installed app's records.
pub(super) const NOT_OURS: &str = "This task's worktree isn't in this Patchwork's worktrees folder";

/// Settings that keep repository-controlled programs from running for Patchwork's own commands.
const SAFE: [&str; 8] = [
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "diff.external=",
    "-c",
    "commit.gpgsign=false",
];

/// Runs `git <args>` in `root` with the repository's hooks and helpers turned off.
pub(in crate::worktree) fn git(root: &str, args: &[&str]) -> Result<String, Error> {
    let mut all = SAFE.to_vec();
    all.extend_from_slice(args);
    source_control::run_git(root, &all)
}

/// Runs `git <args>` in `worktree` with its work tree pinned to it, so a `core.worktree` set in
/// the repository can't turn a commit or a reset onto another folder.
pub(super) fn in_worktree(worktree: &Worktree, args: &[&str]) -> Result<String, Error> {
    let pinned = format!("--work-tree={}", worktree.top);
    let mut all = vec![pinned.as_str()];
    all.extend_from_slice(args);
    git(&worktree.top, &all)
}

/// Whether the checkout at `top` is in the middle of a merge, rebase, cherry-pick or revert.
pub(super) fn in_progress(top: &str) -> bool {
    [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
    ]
    .iter()
    .any(|name| {
        git(
            top,
            &["rev-parse", "--path-format=absolute", "--git-path", name],
        )
        .is_ok_and(|path| Path::new(path.trim()).exists())
    })
}

/// The canonical form of the path git printed.
pub(super) fn canonical(printed: &str) -> Result<PathBuf, Error> {
    std::fs::canonicalize(printed.trim()).map_err(|e| Error::from(e.to_string()))
}

/// The settings that name the committer as Patchwork in a repository at `top` without a name of
/// its own to commit under.
pub(super) fn identity(top: &str) -> Vec<&'static str> {
    if git(top, &["config", "user.email"]).is_ok() {
        return Vec::new();
    }
    vec![
        "-c",
        "user.name=Patchwork",
        "-c",
        "user.email=patchwork@localhost",
    ]
}

/// Waits for, then holds, the lock on merges into the repository at `top`: one merge at a time,
/// so two tasks merged together don't race in the project's checkout, from this app or another
/// Patchwork, such as a development build, open on the same repository. It is a file in the
/// repository's git folder, which the lock leaves behind and the system lets go of with it.
/// `waiting` is called when another merge holds the lock, before waiting for it.
pub(super) fn merge_lock(top: &str, waiting: impl FnOnce()) -> Result<File, Error> {
    let common = git(
        top,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let path = Path::new(common.trim()).join("patchwork-merge.lock");
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => return Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => waiting(),
        Err(std::fs::TryLockError::Error(e)) => {
            return Err(format!("Cannot lock {}: {e}", path.display()).into());
        }
    }
    file.lock()
        .map_err(|e| format!("Cannot lock {}: {e}", path.display()))?;
    Ok(file)
}

/// The main folder of the repository the worktree at `top` belongs to: the first that
/// `git worktree list` names. None for a bare repository, or when git can't tell.
pub(super) fn main_top(top: &str) -> Option<String> {
    let first = listed(top).ok()?.into_iter().next()?;
    (!first.bare && Path::new(&first.path).is_dir()).then_some(first.path)
}

/// One worktree as `git worktree list` names it.
pub(in crate::worktree) struct Listed {
    pub(in crate::worktree) path: String,
    /// The branch checked out there, without `refs/heads/`; None when HEAD is detached.
    pub(in crate::worktree) branch: Option<String>,
    pub(in crate::worktree) bare: bool,
}

/// The worktrees of the repository `top` belongs to, the main one first. Read in the NUL-separated
/// form, so a folder whose name holds a newline can't split a record.
pub(in crate::worktree) fn listed(top: &str) -> Result<Vec<Listed>, Error> {
    let list = git(top, &["worktree", "list", "--porcelain", "-z"])?;
    Ok(list
        .split("\0\0")
        .filter_map(|record| {
            let mut fields = record.split('\0');
            let path = fields.next()?.strip_prefix("worktree ")?.to_owned();
            let mut listed = Listed {
                path,
                branch: None,
                bare: false,
            };
            for field in fields {
                if field == "bare" {
                    listed.bare = true;
                } else if let Some(branch) = field.strip_prefix("branch refs/heads/") {
                    listed.branch = Some(branch.to_owned());
                }
            }
            Some(listed)
        })
        .collect())
}

/// The folder of the worktree in the repository at `top` that has `branch` checked out, if one
/// has.
pub(super) fn checked_out_at(top: &str, branch: &str) -> Result<Option<String>, Error> {
    Ok(listed(top)?
        .into_iter()
        .find(|l| l.branch.as_deref() == Some(branch))
        .map(|l| l.path))
}

/// Whether the worktree at `top` is locked (`git worktree lock`), which keeps git from moving or
/// removing it, so Patchwork leaves it too.
pub(super) fn locked(top: &str) -> bool {
    Path::new(top).is_dir()
        && git(
            top,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "locked",
            ],
        )
        .is_ok_and(|path| Path::new(path.trim()).exists())
}

/// What `worktree` is on instead of its own branch: another branch's name, or "a detached HEAD".
/// None while it is on its branch, or when git can't tell.
pub(super) fn off_branch(worktree: &Worktree) -> Option<String> {
    let on = match in_worktree(worktree, &["symbolic-ref", "-q", "--short", "HEAD"]) {
        Ok(name) => name.trim().to_owned(),
        // `symbolic-ref -q` fails quietly only when HEAD is detached.
        Err(_) => in_worktree(worktree, &["rev-parse", "--verify", "-q", "HEAD"])
            .ok()
            .map(|_| "a detached HEAD".to_owned())?,
    };
    (on != worktree.branch).then_some(on)
}
