//! A task's own git worktree. A task in a git project gets one on its first turn, so tasks that
//! run side by side in a project never edit the same files. Its work reaches the project when the
//! user merges it, and leaves nothing behind when they discard it.
//!
//! Patchwork's own git commands here run with the repository's hooks and file monitor turned
//! off: an agent can write those, and they would otherwise run when the user clicks Merge.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use crate::system::{new_id, ssh};
use crate::threads::{NOT_FOUND, Role, Thread};
use crate::workspace::{self, FileChange};
use crate::{Core, Error};

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
}

/// What a task's worktree holds that its project doesn't have yet.
#[derive(Debug, Clone)]
pub struct WorktreeState {
    pub worktree: Worktree,
    /// Files changed in the worktree and not committed there yet.
    pub changes: Vec<FileChange>,
    /// Commits on the worktree's branch that the project's branch doesn't have.
    pub commits: usize,
}

/// One merge at a time, so two tasks merged together don't race in the project's checkout.
static MERGING: Mutex<()> = Mutex::new(());

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
fn git(root: &str, args: &[&str]) -> Result<String, Error> {
    let mut all = SAFE.to_vec();
    all.extend_from_slice(args);
    workspace::run_git(root, &all)
}

/// Runs `git <args>` in `worktree` with its work tree pinned to it, so a `core.worktree` set in
/// the repository can't turn a commit or a reset onto another folder.
fn in_worktree(worktree: &Worktree, args: &[&str]) -> Result<String, Error> {
    let pinned = format!("--work-tree={}", worktree.top);
    let mut all = vec![pinned.as_str()];
    all.extend_from_slice(args);
    git(&worktree.top, &all)
}

/// Whether the checkout at `top` is in the middle of a merge, rebase, cherry-pick or revert.
fn in_progress(top: &str) -> bool {
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

/// The `app_state` key a task's worktree is stored under.
fn key(thread_id: &str) -> String {
    format!("worktree:{thread_id}")
}

impl Core {
    /// The worktree task `thread_id` works in, while it is one Patchwork made.
    #[must_use]
    pub fn thread_worktree(&self, thread_id: &str) -> Option<Worktree> {
        let stored: Worktree = serde_json::from_str(&self.store.state(&key(thread_id))?).ok()?;
        self.owns(&stored).then_some(stored)
    }

    /// Whether `worktree` is in Patchwork's worktrees folder and its root inside it, so a value
    /// changed in the database can't point a removal or a reset somewhere else.
    fn owns(&self, worktree: &Worktree) -> bool {
        let (Ok(dir), Ok(top)) = (
            std::fs::canonicalize(&self.worktrees_dir),
            std::fs::canonicalize(&worktree.top),
        ) else {
            return false;
        };
        let root_inside = std::fs::canonicalize(&worktree.root).is_ok_and(|r| r.starts_with(&top));
        top.parent() == Some(dir.as_path())
            && root_inside
            && worktree.branch.starts_with("patchwork/")
            && !worktree.branch.contains("..")
    }

    /// Whether the project's tasks edit its own folder rather than a worktree each.
    #[must_use]
    pub fn edits_in_place(&self, project_id: &str) -> bool {
        self.store
            .settings()
            .edit_in_place
            .iter()
            .any(|id| id == project_id)
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
        let (Ok(dir), Ok(path)) = (
            std::fs::canonicalize(&self.worktrees_dir),
            std::fs::canonicalize(path),
        ) else {
            return false;
        };
        path.starts_with(dir)
    }

    /// The folder task `thread_id` works in: its worktree, its project's folder, or its chat's
    /// own folder.
    ///
    /// # Errors
    ///
    /// Returns an error when the task or its project is unknown.
    pub fn thread_root(&self, thread_id: &str) -> Result<String, Error> {
        let thread = self.store.thread(thread_id).ok_or(NOT_FOUND)?;
        let owner = thread
            .parent
            .as_ref()
            .map_or(thread_id, |p| p.thread_id.as_str());
        if let Some(worktree) = self.thread_worktree(owner) {
            return Ok(worktree.root);
        }
        match &thread.project_id {
            Some(project) => self.project_root(project),
            None => Ok(self.chat_dir(thread_id).to_string_lossy().into_owned()),
        }
    }

    /// The folder a turn of `thread`, in the project at `path`, runs in. A subagent works where
    /// the task that started it does. A task's first turn makes its worktree when its project
    /// is a git repository on this computer, unless the project edits in place, the turn only
    /// plans, or the task is a chat app's own chat. Anything that fails leaves it in `path`.
    pub(crate) fn turn_root(&self, thread: &Thread, project_id: &str, path: &str) -> String {
        let owner = thread
            .parent
            .as_ref()
            .map_or(thread.id.as_str(), |p| p.thread_id.as_str());
        if let Some(worktree) = self.thread_worktree(owner) {
            return worktree.root;
        }
        let first_turn = thread.session_id.is_none()
            && !thread.messages.iter().any(|m| m.role == Role::Assistant);
        if thread.parent.is_some()
            || !first_turn
            || thread.plan_mode
            || ssh::is_remote(path)
            || self.edits_in_place(project_id)
            || self.is_worktree_project(path)
            || self.is_dispatcher(&thread.id)
        {
            return path.to_owned();
        }
        match self.add_worktree(&thread.id, path) {
            Ok(Some(worktree)) => worktree.root,
            Ok(None) => path.to_owned(),
            Err(e) => {
                log::warn!(error:% = e; "could not make the task's worktree, so it works in the project: {{error}}");
                path.to_owned()
            }
        }
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
        std::fs::create_dir_all(&self.worktrees_dir)
            .map_err(|e| format!("Cannot create {}: {e}", self.worktrees_dir.display()))?;
        let dir = self.worktrees_dir.join(format!("{name}-{short}"));
        let dir = dir.to_string_lossy().into_owned();
        let branch = format!("patchwork/{short}");
        git(top, &["worktree", "add", "-b", &branch, "--", &dir, "HEAD"])?;
        let worktree = Worktree {
            root: Path::new(&dir)
                .join(prefix.trim())
                .to_string_lossy()
                .trim_end_matches('/')
                .to_owned(),
            top: dir,
            branch,
        };
        self.store.set_state(
            &key(thread_id),
            &serde_json::to_string(&worktree).map_err(|e| e.to_string())?,
        )?;
        log::info!(branch:% = worktree.branch; "made a worktree for a task on {{branch}}");
        Ok(Some(worktree))
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
        let range = format!("{}..HEAD", head.trim());
        let commits = git(&worktree.top, &["rev-list", "--count", &range])?
            .trim()
            .parse()
            .unwrap_or(0);
        Ok(Some(WorktreeState {
            worktree,
            changes,
            commits,
        }))
    }

    /// The top folder of the repository task `thread_id`'s project is in.
    fn project_top(&self, thread_id: &str) -> Result<String, Error> {
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
        let mismatch = || Error::from("This task's worktree is not the one Patchwork made");
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
        let branch = in_worktree(worktree, &["symbolic-ref", "-q", "--short", "HEAD"])?;
        if canonical(&theirs)? != canonical(&ours)?
            || canonical(&shown)? != canonical(&worktree.top)?
            || branch.trim() != worktree.branch
        {
            return Err(mismatch());
        }
        Ok(top)
    }

    /// Whether task `thread_id` or one of its subagents, which work in its worktree, is running.
    fn busy(&self, thread_id: &str) -> bool {
        self.runner.is_running(thread_id)
            || self.store.threads().iter().any(|t| {
                t.parent.as_ref().is_some_and(|p| p.thread_id == thread_id)
                    && self.runner.is_running(&t.id)
            })
    }

    /// Commits whatever the worktree has that isn't committed, as `message`.
    fn commit_worktree(worktree: &Worktree, message: &str) -> Result<(), Error> {
        in_worktree(worktree, &["add", "-A"])?;
        if in_worktree(worktree, &["diff", "--cached", "--quiet"]).is_ok() {
            return Ok(());
        }
        // A repository without a name to commit under still takes Patchwork's.
        let named = git(&worktree.top, &["config", "user.email"]).is_ok();
        let mut args = Vec::new();
        if !named {
            args.extend([
                "-c",
                "user.name=Patchwork",
                "-c",
                "user.email=patchwork@localhost",
            ]);
        }
        args.extend(["commit", "--no-verify", "-q", "-m", message]);
        in_worktree(worktree, &args)?;
        Ok(())
    }

    /// Merges task `thread_id`'s work into the branch its project is on, as a merge commit: what
    /// it hasn't committed is committed first, under the task's title. A merge that would
    /// conflict is undone and leaves the project as it was. Returns the branch merged into.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has no worktree or is still running, when the project has
    /// changes the merge would overwrite, when the merge conflicts, or when git fails.
    pub fn merge_thread(&self, thread_id: &str) -> Result<String, Error> {
        let worktree = self
            .thread_worktree(thread_id)
            .ok_or("This task doesn't have a worktree")?;
        if self.busy(thread_id) {
            return Err("Wait for the task to finish before merging it".into());
        }
        let title = self.thread(thread_id)?.title;
        let _merging = MERGING.lock().unwrap_or_else(PoisonError::into_inner);
        let top = self.verify(thread_id, &worktree)?;
        // Git would refuse to merge anyway, and undoing that refusal would undo the user's own.
        if in_progress(&top) {
            return Err(
                "Finish or abort the merge, rebase or cherry-pick in progress in the project first"
                    .into(),
            );
        }
        Self::commit_worktree(&worktree, &title)?;
        let into = workspace::git_branch(&top).unwrap_or_else(|| "HEAD".into());
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
        let named = git(&top, &["config", "user.email"]).is_ok();
        let mut args = Vec::new();
        if !named {
            args.extend([
                "-c",
                "user.name=Patchwork",
                "-c",
                "user.email=patchwork@localhost",
            ]);
        }
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
        if self.busy(thread_id) {
            return Err("Stop the task before discarding its changes".into());
        }
        let top = self.verify(thread_id, &worktree)?;
        let head = git(&top, &["rev-parse", "HEAD"])?;
        let base = in_worktree(&worktree, &["merge-base", "HEAD", head.trim()])?;
        in_worktree(&worktree, &["reset", "-q", "--hard", base.trim()])?;
        in_worktree(&worktree, &["clean", "-q", "-fd"])?;
        Ok(())
    }

    /// Removes task `thread_id`'s worktree as the task goes. What it hadn't committed is committed
    /// to its branch first, and the branch stays unless the project already has all of it.
    pub(crate) fn remove_thread_worktree(&self, thread_id: &str) {
        let Some(worktree) = self.thread_worktree(thread_id) else {
            return;
        };
        let removed = (|| -> Result<(), Error> {
            let top = self.verify(thread_id, &worktree)?;
            Self::commit_worktree(&worktree, "Work left by a deleted Patchwork task")?;
            let common = git(
                &top,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )?;
            let git_dir = format!("--git-dir={}", common.trim());
            let dir = self.worktrees_dir.to_string_lossy().into_owned();
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
            let merged = git(
                &top,
                &[
                    "merge-base",
                    "--is-ancestor",
                    "--",
                    &worktree.branch,
                    "HEAD",
                ],
            )
            .is_ok();
            if merged {
                git(
                    &dir,
                    &[&git_dir, "branch", "-q", "-D", "--", &worktree.branch],
                )?;
            } else {
                log::info!(branch:% = worktree.branch; "kept the unmerged branch {{branch}} of a deleted task");
            }
            Ok(())
        })();
        // A worktree left behind keeps its record, so the folder and its branch stay found.
        if let Err(e) = removed {
            log::warn!(error:% = e; "could not remove a deleted task's worktree: {{error}}");
            return;
        }
        if let Err(e) = self.store.clear_state(&key(thread_id)) {
            log::warn!(error:% = e; "could not forget a deleted task's worktree: {{error}}");
        }
    }
}
