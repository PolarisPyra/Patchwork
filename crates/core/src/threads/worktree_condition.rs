//! Telling apart what became of a task's worktree: a cleanup rule removed its folder, someone
//! deleted the folder, git removed the worktree, or the folder is there but its link to its
//! repository broke. Each asks for something else, so its record and its repository are read
//! together before a turn or a cleanup acts on it.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};

use super::worktree::{Worktree, git, listed};

/// What became of a task's worktree, as its record, its folder and its repository tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Condition {
    /// A cleanup rule removed its folder and kept its branch; its task's next turn makes it again.
    Parked,
    /// Its folder is there, and is the worktree its repository lists there.
    Present,
    /// Its folder was deleted outside Patchwork, and git still lists the worktree.
    FolderDeleted,
    /// Neither its folder nor git's record of it is left: git removed it, or pruned it once its
    /// folder was deleted.
    Unregistered,
    /// Its folder is there, but isn't a worktree of its repository: its `.git` file is gone or
    /// names nothing, or the repository lists it elsewhere. Git run there may reach another
    /// repository, so nothing commits, resets or removes it.
    Broken,
}

/// What became of `worktree` of the repository whose main folder is `repo`.
pub(super) fn condition(worktree: &Worktree, repo: &str) -> Condition {
    if worktree.parked {
        return Condition::Parked;
    }
    let listed = listed(repo).is_ok_and(|all| {
        all.iter()
            .any(|l| !l.bare && same_folder(&l.path, &worktree.top))
    });
    if !Path::new(&worktree.top).exists() {
        return if listed {
            Condition::FolderDeleted
        } else {
            Condition::Unregistered
        };
    }
    if listed && linked(worktree, repo) {
        Condition::Present
    } else {
        Condition::Broken
    }
}

/// [`condition`], after `git worktree repair` once for a worktree that seems broken, which links
/// a folder and its repository again when either moved.
pub(super) fn repaired_condition(worktree: &Worktree, repo: &str) -> Condition {
    let found = condition(worktree, repo);
    if found != Condition::Broken {
        return found;
    }
    if let Err(e) = git(repo, &["worktree", "repair", "--", &worktree.top]) {
        log::warn!(branch:% = worktree.branch, error:% = e; "could not repair the worktree on {{branch}}: {{error}}");
    }
    condition(worktree, repo)
}

/// Whether git, run in `worktree`'s folder, finds that folder as its top and `repo`'s
/// repository, rather than failing or walking up into a repository around it.
fn linked(worktree: &Worktree, repo: &str) -> bool {
    let args = ["rev-parse", "--path-format=absolute", "--git-common-dir"];
    let (Ok(ours), Ok(theirs), Ok(shown)) = (
        git(&worktree.top, &args),
        git(repo, &args),
        git(&worktree.top, &["rev-parse", "--show-toplevel"]),
    ) else {
        return false;
    };
    same_folder(&ours, &theirs) && same_folder(&shown, &worktree.top)
}

/// Whether `a` and `b` name the same folder, through links; one that is gone by its parent's
/// real path.
fn same_folder(a: &str, b: &str) -> bool {
    matches!((resolve(a), resolve(b)), (Some(a), Some(b)) if a == b)
}

/// The real path of `printed`, or of its parent with its name when it is gone.
fn resolve(printed: &str) -> Option<PathBuf> {
    let path = Path::new(printed.trim());
    std::fs::canonicalize(path).ok().or_else(|| {
        Some(
            std::fs::canonicalize(path.parent()?)
                .ok()?
                .join(path.file_name()?),
        )
    })
}
