//! A task's own git worktree. A task in a git project gets one on its first turn, so tasks that
//! run side by side in a project never edit the same files. Its work reaches the project when the
//! user merges it, and leaves nothing behind when they discard it.
//!
//! Patchwork's own git commands here run with the repository's hooks and file monitor turned
//! off: an agent can write those, and they would otherwise run when the user clicks Merge.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::system::{new_id, ssh};
use crate::threads::{NOT_FOUND, Role, Thread};
use crate::workspace::{self, FileChange, landed};
use crate::{Core, Error};

use super::worktree_condition::{Condition, repaired_condition};
use super::worktree_home::{is_home, make_home};
use super::worktree_include::copy_included;

/// A task's worktree, as stored under its `app_state` key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    /// The folder the task works in: the worktree's copy of the project's folder.
    pub root: String,
    /// The worktree's top folder, the one removing it removes.
    pub top: String,
    /// The branch the worktree is on.
    pub branch: String,
    /// The branch the project was on when the worktree was made, which merging goes back into.
    /// None for a worktree made from a detached HEAD, or stored before this was kept.
    #[serde(default)]
    pub base: Option<String>,
    /// Whether a cleanup rule removed its folder and kept its branch: the task's next turn makes
    /// it again there.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parked: bool,
}

/// What a task's worktree holds that its project doesn't have yet.
#[derive(Debug, Clone)]
pub struct WorktreeState {
    pub worktree: Worktree,
    /// Files changed in the worktree and not committed there yet.
    pub changes: Vec<FileChange>,
    /// Commits on the worktree's branch that neither the project's branch nor the remote's
    /// default branch has.
    pub commits: usize,
    /// Commits on the project's branch that the worktree's branch doesn't have yet.
    pub behind: usize,
    /// What the worktree is on instead of its branch, such as a branch the agent switched to.
    /// Merging and discarding wait until it is switched back.
    pub off_branch: Option<String>,
    /// The branch that already has every file the worktree changed, as the worktree has it, while
    /// the worktree has no commits of its own: cleaning it up loses nothing.
    pub landed_on: Option<String>,
}

/// A task's worktree against the branch it goes back into, for Source Control to show.
#[derive(Debug, Clone)]
pub struct BranchCompare {
    /// The folder compared: the worktree's copy of the project's folder.
    pub root: String,
    /// The branch it goes back into, as the project has it checked out.
    pub base: String,
    /// The commit the worktree's branch left that branch at, which its diffs start from.
    pub since: String,
    /// The commits the worktree's branch made since.
    pub commits: usize,
    /// The files that differ from that commit, committed or not, named from `root`.
    pub files: Vec<FileChange>,
}

/// What a task's worktree needs from the user, as the sidebar marks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeMark {
    /// It has changes or commits the project doesn't have yet.
    Unmerged,
    /// It is on another branch than its own, so it can't be merged until it is switched back.
    OffBranch,
    /// It has nothing the project doesn't have.
    Clean,
}

/// Why a task can't use a worktree its record names outside Patchwork's worktrees folders,
/// such as one a development build read from the installed app's records.
const NOT_OURS: &str = "This task's worktree isn't in this Patchwork's worktrees folder";

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
pub(super) fn git(root: &str, args: &[&str]) -> Result<String, Error> {
    let mut all = SAFE.to_vec();
    all.extend_from_slice(args);
    workspace::run_git(root, &all)
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
fn canonical(printed: &str) -> Result<PathBuf, Error> {
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

/// The commit a new worktree of the repository at `top` starts from: its branch's upstream when
/// that is ahead of HEAD alone, as it is after a fetch the branch hasn't pulled, else HEAD. It is
/// the upstream's commit rather than its name, so the new branch doesn't track it.
fn start_point(top: &str) -> String {
    git(top, &["rev-parse", "--verify", "-q", "@{upstream}"])
        .map(|sha| sha.trim().to_owned())
        .ok()
        .filter(|sha| git(top, &["merge-base", "--is-ancestor", "HEAD", sha]).is_ok())
        .unwrap_or_else(|| "HEAD".to_owned())
}

/// Takes back what a failed `worktree add` of folder `dir` on `branch` left in the repository at
/// `top`: the folder, git's record of it, and the branch. Each step goes ahead when the one
/// before fails, since the add may have stopped anywhere.
fn undo_add(top: &str, dir: &str, branch: &str) {
    let _ = git(top, &["worktree", "remove", "--force", "--", dir]);
    if Path::new(dir).exists()
        && let Err(e) = std::fs::remove_dir_all(dir)
    {
        log::warn!(error:% = e; "could not remove a task worktree that failed to be made: {{error}}");
    }
    let _ = git(top, &["worktree", "prune"]);
    let _ = git(top, &["branch", "-D", "--", branch]);
}

/// The main folder of the repository the worktree at `top` belongs to: the first that
/// `git worktree list` names. None for a bare repository, or when git can't tell.
pub(super) fn main_top(top: &str) -> Option<String> {
    let first = listed(top).ok()?.into_iter().next()?;
    (!first.bare && Path::new(&first.path).is_dir()).then_some(first.path)
}

/// One worktree as `git worktree list` names it.
pub(super) struct Listed {
    pub(super) path: String,
    /// The branch checked out there, without `refs/heads/`; None when HEAD is detached.
    pub(super) branch: Option<String>,
    pub(super) bare: bool,
}

/// The worktrees of the repository `top` belongs to, the main one first. Read in the NUL-separated
/// form, so a folder whose name holds a newline can't split a record.
pub(super) fn listed(top: &str) -> Result<Vec<Listed>, Error> {
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

/// Deletes `branch` from the repository at `top` when it is there and the default branch
/// already has all of it, even under other commits, as a squash-merged pull request leaves it.
/// Whatever branch the project's folder has checked out doesn't count. Returns whether it was
/// deleted.
pub(super) fn delete_if_landed(top: &str, branch: &str) -> Result<bool, Error> {
    let full = format!("refs/heads/{branch}");
    if git(top, &["rev-parse", "--verify", "-q", &full]).is_err() {
        return Ok(false);
    }
    let landed = workspace::base_ref(top).is_some_and(|base| landed(top, &base, &full));
    if !landed {
        return Ok(false);
    }
    git(top, &["branch", "-q", "-D", "--", branch])?;
    Ok(true)
}

/// The Cargo build folder of the worktree whose top folder is `top`: one per worktree, by its
/// folder's name, so builds in worktrees side by side don't wait on each other's lock.
fn build_dir(top: &Path) -> Option<PathBuf> {
    let name = top.file_name()?;
    Some(
        crate::system::dirs()
            .cache
            .join("worktree-builds")
            .join(name),
    )
}

/// How old a folder in the worktrees folder that no record names must be before it is swept.
const SWEEP_AFTER: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// How long the background cleanup waits between runs.
const CLEANUP_EVERY: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// The `app_state` key of why a task's worktree could not be made.
fn failed_key(thread_id: &str) -> String {
    format!("worktree-failed:{thread_id}")
}

/// How many commits on `worktree`'s branch neither the project's branch, at `project_head`, nor
/// the default branch has, the remote's or the local one. The remote is fetched first, every so
/// often. It is none once the branch has landed: its changes are all on the remote's default
/// branch, even under other commits, as a squash-merged pull request leaves them, or GitHub
/// merged a pull request of it as it is.
pub(super) fn unshipped(worktree: &Worktree, project_head: &str) -> Result<usize, Error> {
    branch_unshipped(&worktree.top, "HEAD", &worktree.branch, project_head)
}

/// [`unshipped`] for commit `rev` of `branch`, in the repository at `repo`, which may be the
/// project's own folder when the branch's worktree has none.
pub(super) fn branch_unshipped(
    repo: &str,
    rev: &str,
    branch: &str,
    project_head: &str,
) -> Result<usize, Error> {
    workspace::fetch_remote(repo);
    let base = workspace::base_ref(repo);
    let local = base
        .as_deref()
        .and_then(|base| base.split_once('/'))
        .map(|(_, name)| format!("refs/heads/{name}"))
        .filter(|name| git(repo, &["rev-parse", "--verify", "-q", name]).is_ok());
    let mut args = vec!["rev-list", "--count", rev, "--not", project_head];
    args.extend(base.as_deref());
    args.extend(local.as_deref());
    let count = git(repo, &args)?.trim().parse().unwrap_or(0);
    if count > 0 && landed_branch(repo, rev, branch, base.as_deref()) {
        return Ok(0);
    }
    Ok(count)
}

/// Whether commit `rev` of `branch` has landed on `base`, the default branch: its changes are
/// all there, or GitHub merged a pull request of it as it is.
pub(super) fn landed_branch(repo: &str, rev: &str, branch: &str, base: Option<&str>) -> bool {
    base.is_some_and(|base| landed(repo, base, rev)) || workspace::pr_merged(repo, branch, rev)
}

/// The mark of a worktree whose branch has `unshipped` commits of its own.
fn mark_for(unshipped: usize) -> WorktreeMark {
    if unshipped > 0 {
        WorktreeMark::Unmerged
    } else {
        WorktreeMark::Clean
    }
}

/// The most changed files the check for changes already landed compares, one git call each.
const LANDED_MAX: usize = 50;

/// The first of `candidates`, as (name, commit), whose files match each of `changes`, named from
/// the worktree's top, as the worktree has it. Files are hashed without the repository's
/// filters, which it controls; a filtered file then fails to match, and nothing is cleaned up.
fn changes_on(
    worktree: &Worktree,
    changes: &[FileChange],
    candidates: Vec<(String, String)>,
) -> Option<(String, String)> {
    if changes.is_empty() || changes.len() > LANDED_MAX {
        return None;
    }
    let matches = |rev: &str, change: &FileChange| -> bool {
        let at = format!("{rev}:{}", change.path);
        match change.status.as_str() {
            "deleted" => git(&worktree.top, &["cat-file", "-e", &at]).is_err(),
            "modified" | "added" | "untracked" => {
                let ours = git(
                    &worktree.top,
                    &["hash-object", "--no-filters", "--", &change.path],
                );
                let theirs = git(&worktree.top, &["rev-parse", "--verify", "-q", &at]);
                matches!((ours, theirs), (Ok(a), Ok(b)) if a.trim() == b.trim())
            }
            // A rename's old path and a conflict can't be told apart this way.
            _ => false,
        }
    };
    candidates
        .into_iter()
        .find(|(_, rev)| changes.iter().all(|change| matches(rev, change)))
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

/// A hold on a worktree while a merge, update, discard, switch or removal works on it, shared
/// by every task working there. It lets go as it drops.
pub(crate) struct Step<'a> {
    core: &'a Core,
    /// The worktree's top folder, or the task's id when it has none.
    held: String,
}

impl Drop for Step<'_> {
    fn drop(&mut self) {
        self.core.worktree_steps.locked().remove(&self.held);
    }
}

/// Source Control's hold on the folder it commits or pulls in. It lets go as it drops.
pub(crate) struct SourceHold<'a> {
    _step: Option<Step<'a>>,
    _merging: Option<File>,
}

/// The `app_state` key a task's worktree is stored under.
pub(super) fn key(thread_id: &str) -> String {
    format!("worktree:{thread_id}")
}

/// The `app_state` key that gives a fork's first turn the worktree note, although it carries
/// on its source's session.
fn brief_key(thread_id: &str) -> String {
    format!("worktree-brief:{thread_id}")
}

impl Core {
    /// The worktree task `thread_id` works in, while it is one Patchwork made and its folder is
    /// there.
    #[must_use]
    pub fn thread_worktree(&self, thread_id: &str) -> Option<Worktree> {
        let stored = self.worktree_record(thread_id)?;
        (!stored.parked && self.owns(&stored)).then_some(stored)
    }

    /// Task `thread_id`'s worktree as stored, whether or not its folder is there.
    pub(super) fn worktree_record(&self, thread_id: &str) -> Option<Worktree> {
        serde_json::from_str(&self.store.state(&key(thread_id))?).ok()
    }

    /// The tasks whose records name the worktree at `top`, by id: a task and the forks that
    /// share its worktree.
    pub(super) fn sharers(&self, top: &str) -> Vec<String> {
        self.store
            .state_keys("worktree:")
            .into_iter()
            .filter_map(|record| {
                let stored = self.store.state(&record)?;
                let stored: Worktree = serde_json::from_str(&stored).ok()?;
                let id = record.strip_prefix("worktree:")?;
                (stored.top == top).then(|| id.to_owned())
            })
            .collect()
    }

    /// The task the sidebar marks the worktree at `top` on: of the tasks still there that share
    /// it, the one made first, among those not archived while there are any.
    pub(super) fn mark_owner(&self, top: &str) -> Option<String> {
        self.sharers(top)
            .into_iter()
            .filter_map(|id| self.store.thread(&id))
            .min_by_key(|t| (t.archived_at.is_some(), t.created_at))
            .map(|t| t.id)
    }

    /// Gives fork `to` the worktree of task `from`, so it works in the same folder, on the same
    /// branch. Nothing happens when `from` has no worktree.
    fn share_worktree(&self, from: &str, to: &str) -> Result<(), Error> {
        let Some(stored) = self.store.state(&key(from)) else {
            return Ok(());
        };
        self.store.set_state(&key(to), &stored)
    }

    /// Gives fork `fork_id` of task `source_id`, in project `project_id`, the worktree it works
    /// in: the source's, shared, or with `new`, one of its own from the source's. `new` None
    /// follows the project's setting. A worktree of its own that can't be made leaves it sharing.
    /// Its first message then says where it works.
    pub(crate) fn give_fork_worktree(
        &self,
        source_id: &str,
        fork_id: &str,
        project_id: Option<&str>,
        new: Option<bool>,
    ) -> Result<(), Error> {
        let Some(source) = self.worktree_record(source_id) else {
            return Ok(());
        };
        let new = new.unwrap_or_else(|| {
            project_id.is_some() && self.store.settings().worktrees.fork_into_new
        });
        let own = new
            && !source.parked
            && self.owns(&source)
            && match self.fork_worktree(fork_id, &source) {
                Ok(_) => true,
                Err(e) => {
                    log::warn!(error:% = e; "could not make a fork its own worktree, so it shares its source's: {{error}}");
                    false
                }
            };
        if !own {
            self.share_worktree(source_id, fork_id)?;
        }
        self.store.set_state(&brief_key(fork_id), "1")
    }

    /// Why task `thread_id` works in its project's own folder although it was to get a worktree:
    /// the error making it gave.
    #[must_use]
    pub fn worktree_failure(&self, thread_id: &str) -> Option<String> {
        self.store.state(&failed_key(thread_id))
    }

    /// Whether `worktree` is in a worktrees folder of Patchwork's, a project's or the one in the
    /// app's data folder, and its root inside it, so a value changed in the database can't point
    /// a removal or a reset somewhere else.
    fn owns(&self, worktree: &Worktree) -> bool {
        let Ok(top) = std::fs::canonicalize(&worktree.top) else {
            return false;
        };
        let root_inside = std::fs::canonicalize(&worktree.root).is_ok_and(|r| r.starts_with(&top));
        top.parent()
            .is_some_and(|dir| is_home(dir) || self.is_old_home(dir))
            && root_inside
            && worktree.branch.starts_with("patchwork/")
            && !worktree.branch.contains("..")
    }

    /// Whether the project's tasks edit its own folder rather than a worktree each: worktrees
    /// are turned off, everywhere or for this project.
    #[must_use]
    pub fn edits_in_place(&self, project_id: &str) -> bool {
        let settings = self.store.settings();
        settings.worktrees.disabled || settings.edit_in_place.iter().any(|id| id == project_id)
    }

    /// Sets whether the project's new tasks edit its own folder rather than a worktree each.
    /// Tasks that already have a worktree keep it.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be saved.
    pub fn set_edits_in_place(&self, project_id: &str, in_place: bool) -> Result<(), Error> {
        self.update_settings(|s| {
            s.edit_in_place.retain(|id| id != project_id);
            if in_place {
                s.edit_in_place.push(project_id.to_owned());
            }
        })
    }

    /// Whether `path` is a worktree Patchwork made as a project of its own, which needs no
    /// second worktree.
    fn is_worktree_project(&self, path: &str) -> bool {
        std::fs::canonicalize(path).is_ok_and(|path| self.worktree_top_of(&path).is_some())
    }

    /// The folder task `thread_id` works in: its worktree, its project's folder, or its chat's
    /// own folder.
    ///
    /// # Errors
    ///
    /// Returns an error when the task or its project is unknown, and when the task has a
    /// worktree that isn't there now: its work is on its branch, so the project's own folder
    /// doesn't stand in for it, where restoring a checkpoint or discarding a file would change
    /// the user's checkout.
    pub fn thread_root(&self, thread_id: &str) -> Result<String, Error> {
        let thread = self.store.thread(thread_id).ok_or(NOT_FOUND)?;
        let owner = thread
            .parent
            .as_ref()
            .map_or(thread_id, |p| p.thread_id.as_str());
        if let Some(record) = self.worktree_record(owner) {
            return if record.parked {
                Err(
                    "This task's worktree was cleaned up; send it a message to make it again"
                        .into(),
                )
            } else if self.owns(&record) {
                Ok(record.root)
            } else {
                Err(NOT_OURS.into())
            };
        }
        match &thread.project_id {
            Some(project) => self.project_root(project),
            None => Ok(self.chat_dir(thread_id).to_string_lossy().into_owned()),
        }
    }

    /// The folder a turn of `thread`, in the project at `path`, runs in. A subagent works where
    /// the task that started it does. A task's first turn makes its worktree when its project
    /// is a git repository on this computer, unless the project edits in place, the turn only
    /// plans, or the task is a chat app's own chat; if making it fails, that turn runs in
    /// `path` and the task keeps why.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has a worktree that can't be used or made again, so the
    /// turn doesn't start rather than edit the project's own folder.
    pub(crate) fn turn_root(
        &self,
        thread: &Thread,
        project_id: &str,
        path: &str,
    ) -> Result<String, Error> {
        let owner = thread
            .parent
            .as_ref()
            .map_or(thread.id.as_str(), |p| p.thread_id.as_str());
        // A task that has a worktree never works in the project's folder instead: its work is on
        // its branch, and its agent would carry on editing the user's own checkout.
        if let Some(record) = self.worktree_record(owner) {
            let root = self.ready_worktree(owner, &record)?;
            if self.store.state(&failed_key(owner)).is_some()
                && let Err(e) = self.store.clear_state(&failed_key(owner))
            {
                log::warn!(error:% = e; "could not forget why a task had no worktree: {{error}}");
            }
            return Ok(root);
        }
        let first_turn = thread.session_id.is_none()
            && !thread.messages.iter().any(|m| m.role == Role::Assistant);
        if thread.parent.is_some()
            || !first_turn
            || ssh::is_remote(path)
            || self.edits_in_place(project_id)
            || self.is_worktree_project(path)
            || self.is_dispatcher(&thread.id)
        {
            return Ok(path.to_owned());
        }
        match self.add_worktree(&thread.id, path) {
            Ok(Some(worktree)) => {
                // An earlier first turn's failure no longer says where the task works.
                if self.store.state(&failed_key(&thread.id)).is_some()
                    && let Err(e) = self.store.clear_state(&failed_key(&thread.id))
                {
                    log::warn!(error:% = e; "could not forget why a task had no worktree: {{error}}");
                }
                Ok(worktree.root)
            }
            Ok(None) => Ok(path.to_owned()),
            Err(e) => {
                log::warn!(error:% = e; "could not make the task's worktree, so it works in the project: {{error}}");
                // Kept, so the task shows it edits the project's own folder.
                if let Err(e) = self
                    .store
                    .set_state(&failed_key(&thread.id), &e.to_string())
                {
                    log::warn!(error:% = e; "could not keep why a task has no worktree: {{error}}");
                }
                Ok(path.to_owned())
            }
        }
    }

    /// The folder a turn of task `owner`, whose worktree is `record`, works in. A worktree whose
    /// folder is gone is made again from its branch, whether a cleanup rule, someone outside
    /// Patchwork, or git removed it. One whose link to its repository broke, and that `git
    /// worktree repair` can't link again, fails, and so does one Patchwork didn't make.
    fn ready_worktree(&self, owner: &str, record: &Worktree) -> Result<String, Error> {
        let repo = self.project_top(owner)?;
        let made_again = |why: &str| {
            log::warn!(branch:% = record.branch, why:%; "{{why}} the worktree on {{branch}}; making it again from its branch");
            self.unpark_worktree(owner, record).map(|w| w.root)
        };
        match repaired_condition(record, &repo) {
            Condition::Present if self.owns(record) => Ok(record.root.clone()),
            Condition::Present => Err(NOT_OURS.into()),
            Condition::Parked => self.unpark_worktree(owner, record).map(|w| w.root),
            Condition::FolderDeleted => made_again("someone deleted the folder of"),
            Condition::Unregistered => made_again("git removed"),
            Condition::Broken => Err(format!(
                "This task's worktree at {} is no longer linked to its repository. Move what you \
                 need out of it and delete the folder, and the next turn makes it again",
                record.top
            )
            .into()),
        }
    }

    /// `text` with where task `thread_id` works before it, when it works in its own worktree and
    /// `fresh`, the message starting its agent's session, or it is a fork's first message. An
    /// agent that switches branches there leaves the work where Merge doesn't find it.
    pub(crate) fn worktree_brief(&self, thread_id: &str, text: String, fresh: bool) -> String {
        let forked = self.store.state(&brief_key(thread_id)).is_some();
        if forked && let Err(e) = self.store.clear_state(&brief_key(thread_id)) {
            log::warn!(error:% = e; "could not forget a fork's worktree note: {{error}}");
        }
        let Some(worktree) = self.thread_worktree(thread_id).filter(|_| fresh || forked) else {
            return text;
        };
        format!(
            "<worktree>\nYou work in a git worktree of this project, {}, on the branch {}. The \
             user merges that branch into the project from Patchwork. Stay on it: don't check out, \
             switch to or create another branch here, and don't rebase it onto another. Commit \
             here; to push the work to a branch of its own, run \
             `git push -u origin HEAD:<name>`, so Patchwork finds the pushed branch and its pull \
             request.\n\
             </worktree>\n\n{text}",
            worktree.root, worktree.branch
        )
    }

    /// Makes task `thread_id`'s worktree of the repository `path` is in, on a new branch from its
    /// HEAD, and stores it. None when `path` isn't in a repository with a commit.
    fn add_worktree(&self, thread_id: &str, path: &str) -> Result<Option<Worktree>, Error> {
        let Ok(top) = git(path, &["rev-parse", "--show-toplevel"]) else {
            return Ok(None);
        };
        let top = top.trim();
        if git(top, &["rev-parse", "--verify", "-q", "HEAD"]).is_err() {
            return Ok(None);
        }
        let prefix = git(path, &["rev-parse", "--show-prefix"])?;
        let start = start_point(top);
        self.make_worktree(
            thread_id,
            top,
            prefix.trim(),
            &start,
            workspace::git_branch(top),
        )
        .map(Some)
    }

    /// Makes fork `thread_id` a worktree of its own from `source`, its source task's: a new
    /// branch from its HEAD, with what it hadn't committed, merging back into the same branch.
    fn fork_worktree(&self, thread_id: &str, source: &Worktree) -> Result<Worktree, Error> {
        let top = main_top(&source.top).ok_or("The source task's repository is gone")?;
        let start = in_worktree(source, &["rev-parse", "HEAD"])?;
        // What the source hasn't committed, as a commit no branch holds; empty when it has none.
        let mut args = identity(&source.top);
        args.extend(["stash", "create"]);
        let stash = in_worktree(source, &args)?;
        let prefix = Path::new(&source.root)
            .strip_prefix(&source.top)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let worktree =
            self.make_worktree(thread_id, &top, &prefix, start.trim(), source.base.clone())?;
        if !stash.trim().is_empty()
            && let Err(e) = in_worktree(&worktree, &["stash", "apply", "-q", stash.trim()])
        {
            log::warn!(error:% = e; "could not bring a task's uncommitted changes into its fork's worktree: {{error}}");
        }
        Ok(worktree)
    }

    /// Makes task `thread_id`'s worktree of the repository at `top` on a new branch from
    /// `start`, working in its folder `prefix`, and stores it. `base` is the branch merging goes
    /// back into.
    fn make_worktree(
        &self,
        thread_id: &str,
        top: &str,
        prefix: &str,
        start: &str,
        base: Option<String>,
    ) -> Result<Worktree, Error> {
        let short = &new_id()[..8];
        let name: String = Path::new(top)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .collect();
        let name = name.trim_start_matches(['-', '.']);
        let name = if name.is_empty() { "worktree" } else { name };
        let dir = make_home(top)?.join(format!("{name}-{short}"));
        let dir = dir.to_string_lossy().into_owned();
        let branch = format!("patchwork/{short}");
        let worktree = Worktree {
            root: Path::new(&dir)
                .join(prefix)
                .to_string_lossy()
                .trim_end_matches('/')
                .to_owned(),
            top: dir.clone(),
            branch: branch.clone(),
            base,
            parked: false,
        };
        let made = git(top, &["worktree", "add", "-b", &branch, "--", &dir, start])
            .and_then(|_| serde_json::to_string(&worktree).map_err(|e| e.to_string().into()))
            .and_then(|stored| self.store.set_state(&key(thread_id), &stored));
        if let Err(e) = made {
            undo_add(top, &dir, &branch);
            return Err(e);
        }
        log::info!(branch:% = worktree.branch; "made a worktree for a task on {{branch}}");
        self.prepare_worktree(top, &worktree);
        Ok(worktree)
    }

    /// Gives `worktree`, just checked out from the repository at `top`, what git doesn't: the
    /// ignored files `.worktreeinclude` names, its submodules, and a start on its build.
    fn prepare_worktree(&self, top: &str, worktree: &Worktree) {
        copy_included(top, &worktree.top);
        let mut submodules = workspace::unattended_ssh(top);
        submodules.extend(["submodule", "update", "--init", "--recursive"]);
        if Path::new(&worktree.top).join(".gitmodules").is_file()
            && let Err(e) = git(&worktree.top, &submodules)
        {
            log::warn!(error:% = e; "could not check out a task worktree's submodules: {{error}}");
        }
        self.seed_build(top, &worktree.top);
    }

    /// Makes task `thread_id`'s worktree again, at its folder and on its branch, after a cleanup
    /// rule removed its folder. Every task sharing it gets it back.
    fn unpark_worktree(&self, thread_id: &str, parked: &Worktree) -> Result<Worktree, Error> {
        let at = Path::new(&parked.top);
        if !at
            .parent()
            .is_some_and(|dir| is_home(dir) || self.is_old_home(dir))
            || !parked.branch.starts_with("patchwork/")
            || parked.branch.contains("..")
            || at.exists()
        {
            return Err("This task's worktree is not one Patchwork can make again".into());
        }
        let top = self.project_top(thread_id)?;
        if let Err(e) = git(&top, &["worktree", "prune"]) {
            log::warn!(error:% = e; "could not prune a project's worktrees: {{error}}");
        }
        // Git refuses a second checkout of a branch; say where it is rather than pass that on.
        if let Ok(Some(elsewhere)) = checked_out_at(&top, &parked.branch) {
            return Err(format!(
                "{} is checked out in {elsewhere}; switch that worktree to another branch first",
                parked.branch
            )
            .into());
        }
        let full = format!("refs/heads/{}", parked.branch);
        let made = if git(&top, &["rev-parse", "--verify", "-q", &full]).is_ok() {
            git(
                &top,
                &["worktree", "add", "--", &parked.top, &parked.branch],
            )
        } else {
            log::warn!(branch:% = parked.branch; "the branch {{branch}} is gone; starting it again from the project's HEAD");
            let start = start_point(&top);
            git(
                &top,
                &[
                    "worktree",
                    "add",
                    "-b",
                    &parked.branch,
                    "--",
                    &parked.top,
                    &start,
                ],
            )
        };
        if let Err(e) = made {
            if at.exists() {
                let _ = std::fs::remove_dir_all(at);
            }
            let _ = git(&top, &["worktree", "prune"]);
            return Err(e);
        }
        let worktree = Worktree {
            parked: false,
            ..parked.clone()
        };
        self.prepare_worktree(&top, &worktree);
        let stored = serde_json::to_string(&worktree).map_err(|e| e.to_string())?;
        for id in self.sharers(&parked.top) {
            self.store.set_state(&key(&id), &stored)?;
        }
        if self.store.state(&failed_key(thread_id)).is_some() {
            self.store.clear_state(&failed_key(thread_id))?;
        }
        log::info!(branch:% = worktree.branch; "made a task's worktree on {{branch}} again");
        Ok(worktree)
    }

    /// Starts the build folder of the Rust worktree at `worktree_top` as a copy of the project's
    /// at `top`, made in the background, only where the file system shares the copy's blocks
    /// with the original: a new task then doesn't build from cold, and the copy takes no room
    /// until one of them changes. Elsewhere the worktree builds from cold.
    fn seed_build(&self, top: &str, worktree_top: &str) {
        let Some((_, target)) = self.worktree_env(worktree_top).into_iter().next() else {
            return;
        };
        let (from, target) = (Path::new(top).join("target"), PathBuf::from(target));
        if !from.is_dir() || target.exists() {
            return;
        }
        std::thread::spawn(move || {
            let mut seeding = target.clone().into_os_string();
            seeding.push(".seeding");
            let seeding = PathBuf::from(seeding);
            if let Some(parent) = target.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                log::warn!(error:% = e; "could not make the worktree builds folder: {{error}}");
                return;
            }
            let copied = crate::system::host_command("cp")
                .args(["-a", "--reflink=always", "--"])
                .arg(&from)
                .arg(&seeding)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            // A build started meanwhile keeps its own folder.
            if !copied || std::fs::rename(&seeding, &target).is_err() {
                let _ = std::fs::remove_dir_all(&seeding);
            }
        });
    }

    /// Deletes the Cargo build folder of the worktree at `top`, as the worktree goes.
    fn remove_build(top: &str) {
        let Some(dir) = build_dir(Path::new(top)) else {
            return;
        };
        if dir.is_dir()
            && let Err(e) = std::fs::remove_dir_all(&dir)
        {
            log::warn!(error:% = e; "could not delete a removed worktree's build folder: {{error}}");
        }
    }

    /// The variables the commands run in folder `root` get when it is in a task's worktree of a
    /// Rust project: a Cargo build folder of the worktree's own, outside it, which goes when the
    /// worktree does. None when the user or the project set their own.
    pub(crate) fn worktree_env(&self, root: &str) -> Vec<(&'static str, String)> {
        let Some(top) = std::fs::canonicalize(root)
            .ok()
            .and_then(|root| self.worktree_top_of(&root))
        else {
            return Vec::new();
        };
        let own = crate::system::login_var("CARGO_TARGET_DIR").is_some()
            || ["config.toml", "config"].iter().any(|name| {
                std::fs::read_to_string(top.join(".cargo").join(name))
                    .is_ok_and(|text| text.contains("target-dir"))
            });
        if own || !top.join("Cargo.toml").is_file() {
            return Vec::new();
        }
        build_dir(&top).map_or_else(Vec::new, |target| {
            vec![("CARGO_TARGET_DIR", target.to_string_lossy().into_owned())]
        })
    }

    /// What task `thread_id`'s worktree holds that its project doesn't have yet. None for a task
    /// without one.
    ///
    /// # Errors
    ///
    /// Returns an error when git can't read the worktree or the project.
    pub fn worktree_state(&self, thread_id: &str) -> Result<Option<WorktreeState>, Error> {
        let Some(worktree) = self.thread_worktree(thread_id) else {
            return Ok(None);
        };
        let changes = workspace::git_changes(&worktree.root)?;
        let project = self.project_top(thread_id)?;
        let head = git(&project, &["rev-parse", "HEAD"])?;
        let commits = unshipped(&worktree, head.trim())?;
        let ahead = format!("HEAD..{}", head.trim());
        let behind = git(&worktree.top, &["rev-list", "--count", &ahead])?
            .trim()
            .parse()
            .unwrap_or(0);
        let off_branch = off_branch(&worktree);
        let landed_on = if commits == 0 && off_branch.is_none() && !changes.is_empty() {
            Self::landed_on(&worktree, &project, head.trim())?.map(|(name, _)| name)
        } else {
            None
        };
        Ok(Some(WorktreeState {
            worktree,
            changes,
            commits,
            behind,
            off_branch,
            landed_on,
        }))
    }

    /// Everything task `thread_id`'s worktree changed since its branch left the project's
    /// branch, commits included. None for a task without one.
    ///
    /// # Errors
    ///
    /// Returns an error when git can't read the worktree or the project.
    pub fn branch_compare(&self, thread_id: &str) -> Result<Option<BranchCompare>, Error> {
        let Some(worktree) = self.thread_worktree(thread_id) else {
            return Ok(None);
        };
        let project = self.project_top(thread_id)?;
        let head = git(&project, &["rev-parse", "HEAD"])?;
        let since = git(&worktree.top, &["merge-base", "HEAD", head.trim()])?
            .trim()
            .to_owned();
        let base = workspace::git_branch(&project).unwrap_or_else(|| "the project's HEAD".into());
        let commits = git(
            &worktree.top,
            &["rev-list", "--count", &format!("{since}..HEAD")],
        )?
        .trim()
        .parse()
        .unwrap_or(0);
        let files = workspace::changes_since(&worktree.root, &since)?;
        Ok(Some(BranchCompare {
            root: worktree.root,
            base,
            since,
            commits,
            files,
        }))
    }

    /// The branch, the project's or the remote's default, that has every file `worktree`
    /// changed as it has them, with the commit to reset to. `head` is the project's HEAD.
    fn landed_on(
        worktree: &Worktree,
        project: &str,
        head: &str,
    ) -> Result<Option<(String, String)>, Error> {
        // Every change in the worktree, also those outside the project's folder in it.
        let changes = workspace::git_changes(&worktree.top)?;
        let mut candidates = vec![(
            workspace::git_branch(project).unwrap_or_else(|| "the project's HEAD".into()),
            head.to_owned(),
        )];
        if let Some(base) = workspace::base_ref(&worktree.top)
            && let Ok(rev) = git(&worktree.top, &["rev-parse", "--verify", "-q", &base])
        {
            candidates.push((base, rev.trim().to_owned()));
        }
        Ok(changes_on(worktree, &changes, candidates))
    }

    /// What each task worktree has, by task id: work not merged yet, a branch other than its
    /// own, or nothing at all. A worktree whose folder a cleanup removed is marked by its
    /// branch. A worktree tasks share is marked on the first of them only. It runs git in each
    /// worktree, so it belongs off the UI thread.
    #[must_use]
    pub fn worktree_marks(&self) -> HashMap<String, WorktreeMark> {
        let mut marks = HashMap::new();
        let mut seen = HashSet::new();
        for stored in self.store.state_keys("worktree:") {
            let Some(id) = stored.strip_prefix("worktree:") else {
                continue;
            };
            let Some(record) = self.worktree_record(id) else {
                continue;
            };
            if !seen.insert(record.top.clone()) {
                continue;
            }
            let Some(owner) = self.mark_owner(&record.top) else {
                continue;
            };
            let mark = if record.parked {
                self.parked_mark(&owner, &record)
            } else {
                self.thread_worktree(id)
                    .filter(|w| Path::new(&w.top).is_dir())
                    .and_then(|worktree| self.live_mark(&owner, &worktree))
            };
            if let Some(mark) = mark {
                marks.insert(owner, mark);
            }
        }
        self.mark_unstarted(&mut marks);
        marks
    }

    /// The mark of `worktree`, whose folder is there, as task `owner`'s.
    fn live_mark(&self, owner: &str, worktree: &Worktree) -> Option<WorktreeMark> {
        if off_branch(worktree).is_some() {
            return Some(WorktreeMark::OffBranch);
        }
        let dirty =
            git(&worktree.top, &["status", "--porcelain"]).is_ok_and(|out| !out.trim().is_empty());
        if dirty {
            return Some(WorktreeMark::Unmerged);
        }
        let head = git(&self.project_top(owner).ok()?, &["rev-parse", "HEAD"]).ok()?;
        Some(mark_for(unshipped(worktree, head.trim()).ok()?))
    }

    /// The mark of `worktree`, whose folder a cleanup removed, from its branch in task `owner`'s
    /// project. None for a project on another machine.
    fn parked_mark(&self, owner: &str, worktree: &Worktree) -> Option<WorktreeMark> {
        let repo = self.project_top(owner).ok()?;
        if ssh::is_remote(&repo) {
            return None;
        }
        let branch = format!("refs/heads/{}", worktree.branch);
        let head = git(&repo, &["rev-parse", "HEAD"]).ok()?;
        let count = branch_unshipped(&repo, &branch, &worktree.branch, head.trim()).ok()?;
        Some(mark_for(count))
    }

    /// Marks clean each task whose first turn hasn't run yet and will make its worktree, so it
    /// shows the mark before it has one.
    fn mark_unstarted(&self, marks: &mut HashMap<String, WorktreeMark>) {
        let projects = self.store.settings().projects;
        let mut repos: HashMap<String, bool> = HashMap::new();
        for thread in self.list_threads() {
            let Some(project_id) = thread.project_id.as_deref() else {
                continue;
            };
            if marks.contains_key(&thread.id)
                || thread.archived_at.is_some()
                || thread.parent.is_some()
                || thread.session_id.is_some()
                || thread.messages.iter().any(|m| m.role == Role::Assistant)
                || self.worktree_record(&thread.id).is_some()
                || self.store.state(&failed_key(&thread.id)).is_some()
                || self.edits_in_place(project_id)
                || self.is_dispatcher(&thread.id)
            {
                continue;
            }
            let Some(path) = projects
                .iter()
                .find(|p| p.id == project_id)
                .map(|p| &p.path)
            else {
                continue;
            };
            let repo = *repos.entry(path.clone()).or_insert_with(|| {
                !ssh::is_remote(path)
                    && !self.is_worktree_project(path)
                    && git(path, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok()
            });
            if repo {
                marks.insert(thread.id.clone(), WorktreeMark::Clean);
            }
        }
    }

    /// The top folder of the repository task `thread_id`'s project is in.
    pub(super) fn project_top(&self, thread_id: &str) -> Result<String, Error> {
        let thread = self.store.thread(thread_id).ok_or(NOT_FOUND)?;
        let project = self.project_root(thread.project_id.as_deref().ok_or(NOT_FOUND)?)?;
        Ok(git(&project, &["rev-parse", "--show-toplevel"])?
            .trim()
            .to_owned())
    }

    /// Checks, before a commit, merge, reset or removal, that `worktree` is what its record says:
    /// a worktree of task `thread_id`'s project, on its own branch. A record or a repository
    /// changed since then fails here instead of acting on another folder or branch.
    fn verify(&self, thread_id: &str, worktree: &Worktree) -> Result<String, Error> {
        let top = self.verify_repository(thread_id, worktree)?;
        if let Some(on) = off_branch(worktree) {
            return Err(format!(
                "This task's worktree is on {on}, not its branch {}. Switch it back first",
                worktree.branch
            )
            .into());
        }
        Ok(top)
    }

    /// The part of [`Self::verify`] that holds on any branch: `worktree` is a worktree of task
    /// `thread_id`'s project, at the folder its record names. Returns the project's top folder.
    fn verify_repository(&self, thread_id: &str, worktree: &Worktree) -> Result<String, Error> {
        let top = self.project_top(thread_id)?;
        let theirs = git(
            &top,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let ours = in_worktree(
            worktree,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let shown = in_worktree(worktree, &["rev-parse", "--show-toplevel"])?;
        if canonical(&theirs)? != canonical(&ours)?
            || canonical(&shown)? != canonical(&worktree.top)?
        {
            return Err("This task's worktree is not the one Patchwork made".into());
        }
        Ok(top)
    }

    /// Switches task `thread_id`'s worktree back to its own branch, after its agent switched it
    /// to another. What isn't committed comes along; git refuses when that would be lost.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has no worktree or is still running, when a merge, rebase
    /// or cherry-pick is in progress in the worktree, or when git refuses the switch.
    pub fn restore_worktree_branch(&self, thread_id: &str) -> Result<(), Error> {
        let worktree = self
            .thread_worktree(thread_id)
            .ok_or("This task doesn't have a worktree")?;
        let _step = self.begin_step(
            thread_id,
            "Stop the task and its background commands before switching its branch",
        )?;
        self.switch_back(thread_id, &worktree)
    }

    /// Checks out `worktree`'s own branch, when it is on another.
    fn switch_back(&self, thread_id: &str, worktree: &Worktree) -> Result<(), Error> {
        self.verify_repository(thread_id, worktree)?;
        if off_branch(worktree).is_none() {
            return Ok(());
        }
        if in_progress(&worktree.top) {
            return Err(
                "Finish or abort the merge, rebase or cherry-pick in progress in the task's worktree first"
                    .into(),
            );
        }
        in_worktree(worktree, &["checkout", "-q", &worktree.branch, "--"])?;
        log::info!(branch:% = worktree.branch; "switched a task's worktree back to {{branch}}");
        Ok(())
    }

    /// Whether task `thread_id` or one of its subagents, which work in its worktree, is running,
    /// or still has a background command or task it started running.
    /// A fork sharing the worktree counts as the task.
    pub(super) fn busy(&self, thread_id: &str) -> bool {
        self.worktree_threads(thread_id).iter().any(|t| {
            self.runner.is_running(&t.id)
                || self.runner.is_starting(&t.id)
                || t.messages
                    .iter()
                    .any(|m| m.background.iter().any(|task| task.status.is_live()))
        })
    }

    /// Whether an agent is up in task `thread_id`'s worktree, replying or idle: the task's, a
    /// sharing fork's, or one of their subagents'. A message to an idle agent goes on in the
    /// folder it started in, so the folder stays while it is up.
    pub(super) fn agent_up(&self, thread_id: &str) -> bool {
        self.worktree_threads(thread_id)
            .iter()
            .any(|t| self.runner.has_agent(&t.id))
    }

    /// The tasks working in task `thread_id`'s worktree: it, the forks sharing the worktree, and
    /// their subagents.
    fn worktree_threads(&self, thread_id: &str) -> Vec<crate::threads::Thread> {
        let sharing = self.sharing(thread_id);
        self.store
            .threads()
            .into_iter()
            .filter(|t| {
                sharing.contains(&t.id)
                    || t.parent
                        .as_ref()
                        .is_some_and(|p| sharing.contains(&p.thread_id))
            })
            .collect()
    }

    /// Holds task `thread_id`'s worktree for one step, refused with `while_busy` when the task or
    /// one of its subagents is running or starting. The hold is taken before the check, and a
    /// turn checks for it after claiming its start, so the two can't both go ahead.
    fn begin_step(&self, thread_id: &str, while_busy: &str) -> Result<Step<'_>, Error> {
        let step = self.hold_worktree(thread_id)?;
        if self.busy(thread_id) {
            return Err(while_busy.to_owned().into());
        }
        Ok(step)
    }

    /// The tasks working in task `thread_id`'s worktree, it among them: it and the forks sharing
    /// the worktree. Only it when it has none.
    fn sharing(&self, thread_id: &str) -> Vec<String> {
        let mut ids = self
            .worktree_record(thread_id)
            .map(|w| self.sharers(&w.top))
            .unwrap_or_default();
        if !ids.iter().any(|id| id == thread_id) {
            ids.push(thread_id.to_owned());
        }
        ids
    }

    /// What a step on task `thread_id`'s worktree holds: the worktree's folder, which the forks
    /// sharing it hold too, or the task's id when it has none.
    fn hold_key(&self, thread_id: &str) -> String {
        self.worktree_record(thread_id)
            .map_or_else(|| thread_id.to_owned(), |w| w.top)
    }

    /// Holds task `thread_id`'s worktree, refused while another step holds it.
    pub(super) fn hold_worktree(&self, thread_id: &str) -> Result<Step<'_>, Error> {
        let held = self.hold_key(thread_id);
        let fresh = self.worktree_steps.locked().insert(held.clone());
        if !fresh {
            return Err("Patchwork is already working on this task's worktree".into());
        }
        Ok(Step { core: self, held })
    }

    /// Whether a merge, update, discard, switch or cleanup is working on the worktree `thread`
    /// runs in: its own, or its parent's for a subagent. A turn doesn't start then.
    pub(crate) fn worktree_step_running(&self, thread: &Thread) -> bool {
        let owner = thread
            .parent
            .as_ref()
            .map_or(thread.id.as_str(), |p| p.thread_id.as_str());
        let held = self.hold_key(owner);
        self.worktree_steps.locked().contains(&held)
    }

    /// Holds the worktree thread `thread_id` works in, its own or its parent task's, for a step
    /// that rewrites its files, refused with `while_running` while the task or one of its
    /// subagents is running or starting. None when the thread works in no worktree.
    pub(crate) fn hold_turn_folder(
        &self,
        thread_id: &str,
        while_running: &str,
    ) -> Result<Option<Step<'_>>, Error> {
        let thread = self.store.thread(thread_id).ok_or(NOT_FOUND)?;
        let owner = thread
            .parent
            .as_ref()
            .map_or(thread_id, |p| p.thread_id.as_str());
        if self.thread_worktree(owner).is_none() {
            return Ok(None);
        }
        let step = self.hold_worktree(owner)?;
        let sharing = self.sharing(owner);
        let running = self.store.threads().iter().any(|t| {
            let ours = sharing.contains(&t.id)
                || t.parent
                    .as_ref()
                    .is_some_and(|p| sharing.contains(&p.thread_id));
            ours && (self.runner.is_running(&t.id) || self.runner.is_starting(&t.id))
        });
        if running {
            return Err(while_running.to_owned().into());
        }
        Ok(Some(step))
    }

    /// Holds the folder Source Control commits or pulls in, `root`, for one step. In the open
    /// task `thread_id`'s worktree that is the hold of [`Self::hold_turn_folder`]; in a project's
    /// own folder on this computer, the lock merges into it take, so a task's merge doesn't land
    /// in the middle.
    pub(crate) fn hold_source_folder(
        &self,
        root: &str,
        thread_id: Option<&str>,
        while_running: &str,
    ) -> Result<SourceHold<'_>, Error> {
        let in_worktree = thread_id.is_some_and(|id| {
            let thread = self.store.thread(id);
            let owner = thread
                .as_ref()
                .and_then(|t| t.parent.as_ref())
                .map_or(id, |p| p.thread_id.as_str());
            self.thread_worktree(owner).is_some_and(|w| w.root == root)
        });
        if let Some(id) = thread_id.filter(|_| in_worktree) {
            return Ok(SourceHold {
                _step: self.hold_turn_folder(id, while_running)?,
                _merging: None,
            });
        }
        let merging = if ssh::is_remote(root) {
            None
        } else {
            let top = git(root, &["rev-parse", "--show-toplevel"])?;
            Some(merge_lock(top.trim(), || {})?)
        };
        Ok(SourceHold {
            _step: None,
            _merging: merging,
        })
    }

    /// Commits whatever the worktree has that isn't committed, as `message`. Returns the new
    /// commit, or None when there was nothing to commit.
    fn commit_worktree(worktree: &Worktree, message: &str) -> Result<Option<String>, Error> {
        in_worktree(worktree, &["add", "-A"])?;
        if in_worktree(worktree, &["diff", "--cached", "--quiet"]).is_ok() {
            return Ok(None);
        }
        // A repository without a name to commit under still takes Patchwork's.
        let mut args = identity(&worktree.top);
        args.extend(["commit", "--no-verify", "-q", "-m", message]);
        in_worktree(worktree, &args)?;
        Ok(Some(
            in_worktree(worktree, &["rev-parse", "HEAD"])?
                .trim()
                .to_owned(),
        ))
    }

    /// Records `hash`, committed in the folder `root` by a merge or update, as task
    /// `thread_id`'s commit for the graph. A failure is logged: the commit stands either way.
    fn record_step_commit(&self, root: &str, thread_id: &str, hash: Option<&str>) {
        if let Some(hash) = hash
            && let Err(e) = self.record_commit(root, thread_id, hash)
        {
            log::warn!(hash:%, error:% = e; "could not record the task of commit {{hash}}: {{error}}");
        }
    }

    /// Merges task `thread_id`'s work into the branch its project is on, as a merge commit: what
    /// it hasn't committed is committed first, under the task's title. A merge that would
    /// conflict is undone and leaves the project as it was. Returns the branch merged into.
    /// `waiting` is called when another merge into the same repository, from this app or another
    /// Patchwork, holds it up, so the caller can say what the merge waits for.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has no worktree or is still running, when the project has
    /// changes the merge would overwrite, when the merge conflicts, or when git fails.
    pub fn merge_thread(&self, thread_id: &str, waiting: impl FnOnce()) -> Result<String, Error> {
        let worktree = self
            .thread_worktree(thread_id)
            .ok_or("This task doesn't have a worktree")?;
        let _step = self.begin_step(
            thread_id,
            "Wait for the task and its background commands to finish before merging it",
        )?;
        let title = self.thread(thread_id)?.title;
        let top = self.verify(thread_id, &worktree)?;
        let _merging = merge_lock(&top, waiting)?;
        // Git would refuse to merge anyway, and undoing that refusal would undo the user's own.
        if in_progress(&top) {
            return Err(
                "Finish or abort the merge, rebase or cherry-pick in progress in the project first"
                    .into(),
            );
        }
        // Committing in the middle of the worktree's own merge would record its conflicts.
        if in_progress(&worktree.top) {
            return Err(
                "Finish or abort the merge, rebase or cherry-pick in progress in the task's worktree first"
                    .into(),
            );
        }
        let into = workspace::git_branch(&top)
            .ok_or("The project is on a detached HEAD. Check out the branch to merge into first")?;
        if let Some(base) = worktree.base.as_ref().filter(|base| **base != into) {
            return Err(format!(
                "The project is on {into}, but this task started from {base}. Check out {base} to merge into it"
            )
            .into());
        }
        let committed = Self::commit_worktree(&worktree, &title)?;
        self.record_step_commit(&worktree.root, thread_id, committed.as_deref());
        if git(
            &top,
            &[
                "merge-base",
                "--is-ancestor",
                "--",
                &worktree.branch,
                "HEAD",
            ],
        )
        .is_ok()
        {
            return Ok(into);
        }
        let message = format!("Merge {title}");
        let mut args = identity(&top);
        args.extend([
            "merge",
            "--no-ff",
            "--no-edit",
            "--no-verify",
            "-m",
            &message,
            "--",
            &worktree.branch,
        ]);
        let Err(e) = git(&top, &args) else {
            let merged = git(&top, &["rev-parse", "HEAD"]).ok();
            self.record_step_commit(&top, thread_id, merged.as_deref().map(str::trim));
            return Ok(into);
        };
        if git(&top, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).is_err() {
            return Err(e);
        }
        let files = git(&top, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
        let files: Vec<&str> = files.lines().filter(|l| !l.is_empty()).collect();
        if let Err(abort) = git(&top, &["merge", "--abort"]) {
            log::error!(error:% = abort; "could not undo a conflicted merge: {{error}}");
        }
        Err(format!(
            "Merging would conflict in {}. Ask the task to merge {into} into its branch and resolve them, then merge again",
            files.join(", ")
        )
        .into())
    }

    /// Brings the commits on the project's branch that task `thread_id`'s worktree doesn't have
    /// into its branch, as a merge commit, so merging it later meets no surprises: what it hasn't
    /// committed is committed first, under the task's title. A merge that would conflict is
    /// undone and leaves the worktree as it was. Returns the branch brought in.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has no worktree or is still running, when the merge
    /// conflicts, or when git fails.
    pub fn update_worktree(&self, thread_id: &str) -> Result<String, Error> {
        let worktree = self
            .thread_worktree(thread_id)
            .ok_or("This task doesn't have a worktree")?;
        let _step = self.begin_step(
            thread_id,
            "Wait for the task and its background commands to finish before updating it",
        )?;
        let title = self.thread(thread_id)?.title;
        let top = self.verify(thread_id, &worktree)?;
        if in_progress(&worktree.top) {
            return Err(
                "Finish or abort the merge, rebase or cherry-pick in progress in the task's worktree first"
                    .into(),
            );
        }
        let from = workspace::git_branch(&top).unwrap_or_else(|| "HEAD".into());
        let head = git(&top, &["rev-parse", "HEAD"])?;
        let head = head.trim();
        let committed = Self::commit_worktree(&worktree, &title)?;
        self.record_step_commit(&worktree.root, thread_id, committed.as_deref());
        let before = in_worktree(&worktree, &["rev-parse", "HEAD"])?;
        let message = format!("Merge {from} into {}", worktree.branch);
        let mut args = identity(&worktree.top);
        args.extend(["merge", "--no-edit", "--no-verify", "-m", &message, head]);
        let Err(e) = in_worktree(&worktree, &args) else {
            // Only a merge commit on top of the branch is the update's own; already up to date
            // or fast-forwarded, it made none.
            let parent = in_worktree(&worktree, &["rev-parse", "-q", "--verify", "HEAD^1"]);
            if parent.is_ok_and(|p| p.trim() == before.trim()) {
                let updated = in_worktree(&worktree, &["rev-parse", "HEAD"]).ok();
                self.record_step_commit(
                    &worktree.root,
                    thread_id,
                    updated.as_deref().map(str::trim),
                );
            }
            return Ok(from);
        };
        if in_worktree(&worktree, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).is_err() {
            return Err(e);
        }
        let files =
            in_worktree(&worktree, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
        let files: Vec<&str> = files.lines().filter(|l| !l.is_empty()).collect();
        if let Err(abort) = in_worktree(&worktree, &["merge", "--abort"]) {
            log::error!(error:% = abort; "could not undo a conflicted update: {{error}}");
        }
        Err(format!(
            "Updating would conflict in {}. Ask the task to merge {from} into its branch and resolve them",
            files.join(", ")
        )
        .into())
    }

    /// Throws away everything task `thread_id` changed in its worktree that its project doesn't
    /// have: uncommitted files, and commits not merged. Ignored files, such as builds, stay.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has no worktree or is still running, or when git fails.
    pub fn discard_thread_changes(&self, thread_id: &str) -> Result<(), Error> {
        let worktree = self
            .thread_worktree(thread_id)
            .ok_or("This task doesn't have a worktree")?;
        let _step = self.begin_step(
            thread_id,
            "Stop the task and its background commands before discarding its changes",
        )?;
        let top = self.verify(thread_id, &worktree)?;
        let head = git(&top, &["rev-parse", "HEAD"])?;
        let base = in_worktree(&worktree, &["merge-base", "HEAD", head.trim()])?;
        in_worktree(&worktree, &["reset", "-q", "--hard", base.trim()])?;
        in_worktree(&worktree, &["clean", "-q", "-fd"])?;
        Ok(())
    }

    /// Resets task `thread_id`'s worktree to the branch that already has every file it changed,
    /// once it has no commits of its own, so it stops showing changes the project has. Returns
    /// that branch.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has no worktree or is still running, when the worktree has
    /// commits or changes the project doesn't, or when git fails.
    pub fn clean_up_worktree(&self, thread_id: &str) -> Result<String, Error> {
        let worktree = self
            .thread_worktree(thread_id)
            .ok_or("This task doesn't have a worktree")?;
        let _step = self.begin_step(
            thread_id,
            "Wait for the task and its background commands to finish before cleaning it up",
        )?;
        let top = self.verify(thread_id, &worktree)?;
        if in_progress(&worktree.top) {
            return Err(
                "Finish or abort the merge, rebase or cherry-pick in progress in the task's worktree first"
                    .into(),
            );
        }
        let head = git(&top, &["rev-parse", "HEAD"])?;
        if unshipped(&worktree, head.trim())? > 0 {
            return Err("This task's worktree has commits the project doesn't have yet".into());
        }
        let (on, rev) = Self::landed_on(&worktree, &top, head.trim())?
            .ok_or("Some of this task's changes aren't on the project's branch. Merge or discard them instead")?;
        in_worktree(&worktree, &["reset", "-q", "--hard", &rev])?;
        log::info!(branch:% = worktree.branch, on:%; "cleaned up {{branch}}, whose changes {{on}} has");
        Ok(on)
    }

    /// Tidies task worktrees in the background as the app starts. Git forgets the worktrees
    /// whose folders are gone from each project's repository, and Patchwork their records, with
    /// their branches once the project has all of them. A deleted task's worktree that couldn't
    /// be removed then is removed now, and so is a clean worktree no task's record names. Work
    /// a project doesn't have yet stays, on its branch. All of it runs again every hour.
    pub fn start_worktree_cleanup(self: &Arc<Self>) {
        if !self.store.owns_data() {
            return;
        }
        let core = Arc::clone(self);
        std::thread::spawn(move || {
            loop {
                core.clean_worktrees();
                std::thread::sleep(CLEANUP_EVERY);
            }
        });
    }

    fn clean_worktrees(&self) {
        self.move_old_worktrees();
        let mut tops = Vec::new();
        for project in self.store.settings().projects {
            if ssh::is_remote(&project.path) || !Path::new(&project.path).is_dir() {
                continue;
            }
            let Ok(top) = git(&project.path, &["rev-parse", "--show-toplevel"]) else {
                continue;
            };
            // Once a project's folder moves, its worktrees' links still name the old place;
            // run before `prune`, which would otherwise take those worktrees for gone.
            if let Err(e) = git(&project.path, &["worktree", "repair"]) {
                log::warn!(error:% = e; "could not repair a project's worktree links: {{error}}");
            }
            tops.push(top.trim().to_owned());
        }
        // Told apart before `prune`, after which a deleted folder looks like a worktree git
        // removed.
        let records: Vec<(String, Worktree, Option<Condition>)> = self
            .store
            .state_keys("worktree:")
            .into_iter()
            .filter_map(|record| {
                let stored = self.store.state(&record)?;
                let stored = serde_json::from_str::<Worktree>(&stored).ok()?;
                let thread_id = record.strip_prefix("worktree:").unwrap_or(&record);
                let found = (!stored.parked && self.store.thread(thread_id).is_some())
                    .then(|| self.project_top(thread_id).ok())
                    .flatten()
                    .map(|repo| repaired_condition(&stored, &repo));
                Some((record, stored, found))
            })
            .collect();
        for top in &tops {
            if let Err(e) = git(top, &["worktree", "prune"]) {
                log::warn!(error:% = e; "could not prune a project's worktrees: {{error}}");
            }
        }
        let mut recorded = Vec::new();
        for (record, stored, found) in records {
            let thread_id = record.strip_prefix("worktree:").unwrap_or(&record);
            // A cleanup rule removed its folder; its task makes it again, so the record stays.
            if stored.parked
                && (self.store.thread(thread_id).is_some()
                    || self.shared_with_others(&stored.top, thread_id))
            {
                continue;
            }
            match found {
                Some(Condition::Broken) => {
                    log::warn!(branch:% = stored.branch; "the worktree on {{branch}} is no longer linked to its repository, and repair couldn't link it; it stays as it is");
                }
                // Its task's next turn makes it again from its branch.
                Some(gone @ (Condition::FolderDeleted | Condition::Unregistered)) => {
                    let why = if gone == Condition::FolderDeleted {
                        "someone deleted the folder of"
                    } else {
                        "git removed"
                    };
                    // Taken before the busy check, as a turn claims its start before it makes
                    // the worktree again; a record that changed since, or a folder that is back,
                    // is a turn's that made it.
                    let Ok(_step) = self.hold_worktree(thread_id) else {
                        continue;
                    };
                    if self.busy(thread_id)
                        || self.agent_up(thread_id)
                        || self.worktree_record(thread_id).as_ref() != Some(&stored)
                        || Path::new(&stored.top).exists()
                    {
                        continue;
                    }
                    match self.store_parked(&stored) {
                        Ok(()) => {
                            log::info!(branch:% = stored.branch, why:%; "{{why}} the worktree on {{branch}}; its task makes it again from its branch");
                        }
                        Err(e) => {
                            log::warn!(error:% = e; "could not keep a missing worktree to make again: {{error}}");
                        }
                    }
                    continue;
                }
                // Its project's repository can't be read now, so what became of the worktree
                // waits for it; forgetting the record would leave the task in the project's folder.
                None if self.store.thread(thread_id).is_some() => {
                    if !Path::new(&stored.top).exists() {
                        continue;
                    }
                }
                Some(Condition::Present | Condition::Parked) | None => {}
            }
            if let Ok(top) = std::fs::canonicalize(&stored.top) {
                recorded.push(top);
                if self.store.thread(thread_id).is_none() {
                    self.remove_orphan(thread_id, &stored);
                }
                continue;
            }
            // Its folder is gone; its branch goes too once the project has all of it.
            if stored.branch.starts_with("patchwork/") && !stored.branch.contains("..") {
                for top in &tops {
                    if let Err(e) = delete_if_landed(top, &stored.branch) {
                        log::warn!(error:% = e; "could not delete a missing worktree's branch: {{error}}");
                    }
                }
            }
            match self.store.clear_state(&record) {
                Ok(()) => {
                    log::info!(branch:% = stored.branch; "forgot the worktree on {{branch}}, whose folder is gone");
                }
                Err(e) => log::warn!(error:% = e; "could not forget a missing worktree: {{error}}"),
            }
        }
        self.sweep_unrecorded(&self.worktrees_dir, &recorded);
        for top in &tops {
            self.sweep_unrecorded(&super::worktree_home::home(top), &recorded);
        }
        self.settle_landed_worktrees();
    }

    /// Removes task `thread_id`'s worktree as the task goes. What it hadn't committed is committed
    /// to its branch first, and the branch stays unless the project, or the remote's default
    /// branch, already has all of it.
    ///
    /// The worktree stays while another task shares it, and when the project keeps the worktrees
    /// of deleted tasks; the record stays with it then.
    pub(crate) fn remove_thread_worktree(&self, thread_id: &str) {
        let Some(record) = self.worktree_record(thread_id) else {
            return;
        };
        if self.shared_with_others(&record.top, thread_id) {
            self.forget_worktree(thread_id);
            return;
        }
        if !self.removes_with_tasks() {
            return;
        }
        if record.parked {
            // Only its branch is left, which goes once the project has all of it.
            if let Ok(top) = self.project_top(thread_id)
                && let Err(e) = delete_if_landed(&top, &record.branch)
            {
                log::warn!(error:% = e; "could not delete a deleted task's branch: {{error}}");
            }
            self.forget_worktree(thread_id);
            return;
        }
        let Some(worktree) = self.thread_worktree(thread_id) else {
            return;
        };
        let removed = (|| -> Result<(), Error> {
            let _step = self.hold_worktree(thread_id)?;
            // Committing in the middle of a merge would record its conflicts; the worktree stays.
            if in_progress(&worktree.top) {
                return Err("a merge, rebase or cherry-pick is in progress in it".into());
            }
            // Its agent may have left it on another branch; what it holds goes to its own.
            self.switch_back(thread_id, &worktree)?;
            let top = self.verify(thread_id, &worktree)?;
            Self::drop_worktree(&worktree, &top)
        })();
        // A worktree left behind keeps its record, so the folder and its branch stay found, and
        // the next start tries again.
        if let Err(e) = removed {
            log::warn!(error:% = e; "could not remove a deleted task's worktree: {{error}}");
            return;
        }
        // Records of tasks deleted before it name the folder that is gone now.
        for id in self.sharers(&worktree.top) {
            self.forget_worktree(&id);
        }
    }

    /// Forgets task `thread_id`'s record of its worktree.
    pub(super) fn forget_worktree(&self, thread_id: &str) {
        if let Err(e) = self.store.clear_state(&key(thread_id)) {
            log::warn!(error:% = e; "could not forget a task's worktree: {{error}}");
        }
    }

    /// Whether a task other than `thread_id`, still there, shares the worktree at `top`.
    fn shared_with_others(&self, top: &str, thread_id: &str) -> bool {
        self.sharers(top)
            .iter()
            .any(|id| id != thread_id && self.store.thread(id).is_some())
    }

    /// Whether deleting a task removes its worktree.
    fn removes_with_tasks(&self) -> bool {
        self.store.settings().worktrees.cleanup.with_deleted_tasks
    }

    /// Removes `worktree` of the repository whose main folder is `top`: what it hadn't committed
    /// is committed to its branch first, and the branch goes too once `top` has all of it.
    pub(super) fn drop_worktree(worktree: &Worktree, top: &str) -> Result<(), Error> {
        Self::remove_worktree_folder(worktree, top, "Work left by a deleted Patchwork task")?;
        if !delete_if_landed(top, &worktree.branch)? {
            log::info!(branch:% = worktree.branch; "kept the unmerged branch {{branch}} of a removed worktree");
        }
        Ok(())
    }

    /// Removes the folder of `worktree` of the repository whose main folder is `top`, which a
    /// cleanup rule chose, and keeps its branch and every sharing task's record of it, so the
    /// task's next turn makes it again. What it hadn't committed is committed to its branch.
    pub(super) fn park_worktree(&self, worktree: &Worktree, top: &str) -> Result<(), Error> {
        Self::remove_worktree_folder(
            worktree,
            top,
            "Work left by a cleaned-up Patchwork worktree",
        )?;
        self.store_parked(worktree)?;
        log::info!(branch:% = worktree.branch; "removed the folder of the worktree on {{branch}}, keeping the branch");
        Ok(())
    }

    /// Records `worktree`, whose folder is gone, as parked for every task sharing it, so the
    /// task's next turn makes it again from its branch.
    fn store_parked(&self, worktree: &Worktree) -> Result<(), Error> {
        let parked = Worktree {
            parked: true,
            ..worktree.clone()
        };
        let stored = serde_json::to_string(&parked).map_err(|e| e.to_string())?;
        for id in self.sharers(&worktree.top) {
            self.store.set_state(&key(&id), &stored)?;
        }
        Ok(())
    }

    /// Commits what `worktree` hadn't committed, as `message`, and removes its folder and its
    /// build folder. A locked worktree stays as it is. `top` is its repository's main folder.
    fn remove_worktree_folder(worktree: &Worktree, top: &str, message: &str) -> Result<(), Error> {
        if locked(&worktree.top) {
            return Err("it is locked; `git worktree unlock` it to let Patchwork remove it".into());
        }
        Self::commit_worktree(worktree, message)?;
        let common = git(
            top,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let git_dir = format!("--git-dir={}", common.trim());
        // Run from the folder it is in, which stays when it goes.
        let dir = Path::new(&worktree.top)
            .parent()
            .ok_or("the worktree has no folder above it")?
            .to_string_lossy()
            .into_owned();
        git(
            &dir,
            &[
                &git_dir,
                "worktree",
                "remove",
                "--force",
                "--",
                &worktree.top,
            ],
        )?;
        Self::remove_build(&worktree.top);
        Ok(())
    }

    /// Removes the worktree of task `thread_id`, which was deleted while its worktree couldn't be
    /// removed, and forgets it. A worktree it can't remove yet stays, with its record, and so
    /// does one another task shares or its project keeps.
    fn remove_orphan(&self, thread_id: &str, worktree: &Worktree) {
        if self.shared_with_others(&worktree.top, thread_id) {
            self.forget_worktree(thread_id);
            return;
        }
        if !self.owns(worktree) || !self.removes_with_tasks() {
            return;
        }
        let removed = (|| -> Result<(), Error> {
            let _step = self.hold_worktree(thread_id)?;
            let top = main_top(&worktree.top).ok_or("its repository is gone")?;
            let shown = in_worktree(worktree, &["rev-parse", "--show-toplevel"])?;
            if canonical(&shown)? != canonical(&worktree.top)? {
                return Err("it is not the worktree Patchwork made".into());
            }
            if in_progress(&worktree.top) {
                return Err("a merge, rebase or cherry-pick is in progress in it".into());
            }
            if off_branch(worktree).is_some() {
                in_worktree(worktree, &["checkout", "-q", &worktree.branch, "--"])?;
            }
            Self::drop_worktree(worktree, &top)
        })();
        if let Err(e) = removed {
            log::warn!(branch:% = worktree.branch, error:% = e; "could not remove the worktree on {{branch}} of a deleted task: {{error}}");
            return;
        }
        log::info!(branch:% = worktree.branch; "removed the worktree on {{branch}} of a deleted task");
        for id in self.sharers(&worktree.top) {
            self.forget_worktree(&id);
        }
    }

    /// Removes the folders in the worktrees folder `dir` that no task's record names, as a
    /// worktree that failed to be made or whose record was lost leaves: only a worktree on a
    /// `patchwork/` branch with nothing uncommitted, not locked, which no project is in, and not
    /// one made in the last hour, which may be a task's being made now. Its branch stays unless
    /// its repository already has all of it. `recorded` holds the canonical folders records name.
    fn sweep_unrecorded(&self, dir: &Path, recorded: &[PathBuf]) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let projects: Vec<PathBuf> = self
            .store
            .settings()
            .projects
            .iter()
            .filter_map(|p| std::fs::canonicalize(&p.path).ok())
            .collect();
        for entry in entries.flatten() {
            let Ok(path) = std::fs::canonicalize(entry.path()) else {
                continue;
            };
            let recent = entry
                .metadata()
                .and_then(|m| m.modified())
                .map_or(true, |at| {
                    at.elapsed().map_or(true, |age| age < SWEEP_AFTER)
                });
            if !path.is_dir()
                || recent
                || recorded.contains(&path)
                || locked(&path.to_string_lossy())
                || projects.iter().any(|p| p.starts_with(&path))
            {
                continue;
            }
            let top = path.to_string_lossy().into_owned();
            let Ok(branch) = git(&top, &["symbolic-ref", "-q", "--short", "HEAD"]) else {
                continue;
            };
            let branch = branch.trim();
            let own_top = git(&top, &["rev-parse", "--show-toplevel"])
                .ok()
                .and_then(|shown| canonical(&shown).ok())
                .is_some_and(|shown| shown == path);
            let clean = git(&top, &["status", "--porcelain", "--untracked-files=all"])
                .is_ok_and(|status| status.trim().is_empty());
            let Some(main) = main_top(&top).filter(|main| canonical(main).is_ok_and(|m| m != path))
            else {
                continue;
            };
            if !branch.starts_with("patchwork/") || branch.contains("..") || !own_top || !clean {
                continue;
            }
            let worktree = Worktree {
                root: top.clone(),
                top,
                branch: branch.to_owned(),
                base: None,
                parked: false,
            };
            match Self::drop_worktree(&worktree, &main) {
                Ok(()) => {
                    log::info!(branch:% = worktree.branch; "removed a worktree on {{branch}} no task had");
                }
                Err(e) => {
                    log::warn!(branch:% = worktree.branch, error:% = e; "could not remove a worktree on {{branch}} no task had: {{error}}");
                }
            }
        }
    }
}
