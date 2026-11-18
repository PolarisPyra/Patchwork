//! A task's own git worktree. A task in a git project gets one on its first turn, so tasks that
//! run side by side in a project never edit the same files. Its work reaches the project when the
//! user merges it, and leaves nothing behind when they discard it.
//!
//! Patchwork's own git commands here run with the repository's hooks and file monitor turned
//! off: an agent can write those, and they would otherwise run when the user clicks Merge.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::fs::File;
use std::sync::Arc;

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
    /// Commits on the project's branch that the worktree's branch doesn't have yet.
    pub behind: usize,
    /// What the worktree is on instead of its branch, such as a branch the agent switched to.
    /// Merging and discarding wait until it is switched back.
    pub off_branch: Option<String>,
}

/// What a task's worktree needs from the user, as the sidebar marks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeMark {
    /// It has changes or commits the project doesn't have yet.
    Unmerged,
    /// It is on another branch than its own, so it can't be merged until it is switched back.
    OffBranch,
}

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

/// The settings that name the committer as Patchwork in a repository at `top` without a name of
/// its own to commit under.
fn identity(top: &str) -> Vec<&'static str> {
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
fn merge_lock(top: &str, waiting: impl FnOnce()) -> Result<File, Error> {
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

/// Copies into a new worktree at `to` the files git ignores in the repository at `top` that its
/// `.worktreeinclude` names, in `.gitignore`'s form, such as `.env`: a worktree starts with only
/// what git tracks. A file that can't be copied is logged and left out.
fn copy_included(top: &str, to: &str) {
    let list = Path::new(top).join(".worktreeinclude");
    if !list.is_file() {
        return;
    }
    let from = format!("--exclude-from={}", list.display());
    let named = match git(top, &["ls-files", "-z", "--others", "--ignored", &from]) {
        Ok(named) => named,
        Err(e) => {
            log::warn!(error:% = e; "could not read the project's .worktreeinclude: {{error}}");
            return;
        }
    };
    for rel in named.split('\0').filter(|p| !p.is_empty()) {
        // Only what git ignores: a file it tracks is in the worktree already.
        if git(top, &["check-ignore", "-q", "--", rel]).is_err() {
            continue;
        }
        let target = Path::new(to).join(rel);
        let copied = target
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::copy(Path::new(top).join(rel), &target));
        if let Err(e) = copied {
            log::warn!(file:% = rel, error:% = e; "could not copy {{file}} into a task's worktree: {{error}}");
        }
    }
}

/// The `app_state` key of why a task's worktree could not be made.
fn failed_key(thread_id: &str) -> String {
    format!("worktree-failed:{thread_id}")
}

/// What `worktree` is on instead of its own branch: another branch's name, or "a detached HEAD".
/// None while it is on its branch, or when git can't tell.
fn off_branch(worktree: &Worktree) -> Option<String> {
    let on = match in_worktree(worktree, &["symbolic-ref", "-q", "--short", "HEAD"]) {
        Ok(name) => name.trim().to_owned(),
        // `symbolic-ref -q` fails quietly only when HEAD is detached.
        Err(_) => in_worktree(worktree, &["rev-parse", "--verify", "-q", "HEAD"])
            .ok()
            .map(|_| "a detached HEAD".to_owned())?,
    };
    (on != worktree.branch).then_some(on)
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

    /// Why task `thread_id` works in its project's own folder although it was to get a worktree:
    /// the error making it gave.
    #[must_use]
    pub fn worktree_failure(&self, thread_id: &str) -> Option<String> {
        self.store.state(&failed_key(thread_id))
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
                // Kept, so the task shows it edits the project's own folder.
                if let Err(e) = self
                    .store
                    .set_state(&failed_key(&thread.id), &e.to_string())
                {
                    log::warn!(error:% = e; "could not keep why a task has no worktree: {{error}}");
                }
                path.to_owned()
            }
        }
    }

    /// `text` with where task `thread_id` works before it, when it works in its own worktree and
    /// `fresh`: the message starts its agent's session. An agent that switches branches there
    /// leaves the work where Merge doesn't find it.
    pub(crate) fn worktree_brief(&self, thread_id: &str, text: String, fresh: bool) -> String {
        let Some(worktree) = self.thread_worktree(thread_id).filter(|_| fresh) else {
            return text;
        };
        format!(
            "<worktree>\nYou work in a git worktree of this project, {}, on the branch {}. The \
             user merges that branch into the project from Patchwork. Stay on it: don't check out, \
             switch to or create another branch here, and don't rebase it onto another. Commit \
             here; to push the work to a branch of its own, run `git push origin HEAD:<name>`.\n\
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
        copy_included(top, &worktree.top);
        Ok(Some(worktree))
    }

    /// The variables the commands run in folder `root` get when it is in a task's worktree of a
    /// Rust project: one Cargo build folder that the project's worktrees share, so a new task
    /// doesn't start from a cold build. None when the user or the project set their own.
    pub(crate) fn worktree_env(&self, root: &str) -> Vec<(&'static str, String)> {
        let (Ok(dir), Ok(root)) = (
            std::fs::canonicalize(&self.worktrees_dir),
            std::fs::canonicalize(root),
        ) else {
            return Vec::new();
        };
        let Ok(inside) = root.strip_prefix(&dir) else {
            return Vec::new();
        };
        let Some(folder) = inside.components().next() else {
            return Vec::new();
        };
        let top = dir.join(folder);
        let own = crate::system::login_var("CARGO_TARGET_DIR").is_some()
            || ["config.toml", "config"].iter().any(|name| {
                std::fs::read_to_string(top.join(".cargo").join(name))
                    .is_ok_and(|text| text.contains("target-dir"))
            });
        if own || !top.join("Cargo.toml").is_file() {
            return Vec::new();
        }
        // The worktree's folder is "<project>-<id>"; the build is shared by the project's name.
        let folder = folder.as_os_str().to_string_lossy();
        let project = folder.rsplit_once('-').map_or(&*folder, |(name, _)| name);
        let target = crate::system::dirs()
            .cache
            .join("worktree-builds")
            .join(project);
        vec![("CARGO_TARGET_DIR", target.to_string_lossy().into_owned())]
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
        let ahead = format!("HEAD..{}", head.trim());
        let behind = git(&worktree.top, &["rev-list", "--count", &ahead])?
            .trim()
            .parse()
            .unwrap_or(0);
        let off_branch = off_branch(&worktree);
        Ok(Some(WorktreeState {
            worktree,
            changes,
            commits,
            behind,
            off_branch,
        }))
    }

    /// What each task worktree that needs the user has, by task id: work not merged yet, or a
    /// branch other than its own. Tasks whose worktree needs nothing are left out. It runs git in
    /// each worktree, so it belongs off the UI thread.
    #[must_use]
    pub fn worktree_marks(&self) -> HashMap<String, WorktreeMark> {
        let mut marks = HashMap::new();
        for stored in self.store.state_keys("worktree:") {
            let Some(id) = stored.strip_prefix("worktree:") else {
                continue;
            };
            let Some(worktree) = self.thread_worktree(id) else {
                continue;
            };
            if !Path::new(&worktree.top).is_dir() {
                continue;
            }
            if off_branch(&worktree).is_some() {
                marks.insert(id.to_owned(), WorktreeMark::OffBranch);
                continue;
            }
            let dirty = git(&worktree.top, &["status", "--porcelain"])
                .is_ok_and(|out| !out.trim().is_empty());
            let ahead = || {
                let head = git(&self.project_top(id).ok()?, &["rev-parse", "HEAD"]).ok()?;
                let range = format!("{}..HEAD", head.trim());
                let count = git(&worktree.top, &["rev-list", "--count", &range]).ok()?;
                Some(count.trim() != "0")
            };
            if dirty || ahead().unwrap_or(false) {
                marks.insert(id.to_owned(), WorktreeMark::Unmerged);
            }
        }
        marks
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
        if self.busy(thread_id) {
            return Err(
                "Stop the task and its background commands before switching its branch".into(),
            );
        }
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
    fn busy(&self, thread_id: &str) -> bool {
        self.store.threads().iter().any(|t| {
            let ours =
                t.id == thread_id || t.parent.as_ref().is_some_and(|p| p.thread_id == thread_id);
            ours && (self.runner.is_running(&t.id)
                || t.messages
                    .iter()
                    .any(|m| m.background.iter().any(|task| task.status.is_live())))
        })
    }

    /// Commits whatever the worktree has that isn't committed, as `message`.
    fn commit_worktree(worktree: &Worktree, message: &str) -> Result<(), Error> {
        in_worktree(worktree, &["add", "-A"])?;
        if in_worktree(worktree, &["diff", "--cached", "--quiet"]).is_ok() {
            return Ok(());
        }
        // A repository without a name to commit under still takes Patchwork's.
        let mut args = identity(&worktree.top);
        args.extend(["commit", "--no-verify", "-q", "-m", message]);
        in_worktree(worktree, &args)?;
        Ok(())
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
        if self.busy(thread_id) {
            return Err(
                "Wait for the task and its background commands to finish before merging it".into(),
            );
        }
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
        if self.busy(thread_id) {
            return Err(
                "Wait for the task and its background commands to finish before updating it".into(),
            );
        }
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
        Self::commit_worktree(&worktree, &title)?;
        let message = format!("Merge {from} into {}", worktree.branch);
        let mut args = identity(&worktree.top);
        args.extend(["merge", "--no-edit", "--no-verify", "-m", &message, head]);
        let Err(e) = in_worktree(&worktree, &args) else {
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
        if self.busy(thread_id) {
            return Err(
                "Stop the task and its background commands before discarding its changes".into(),
            );
        }
        let top = self.verify(thread_id, &worktree)?;
        let head = git(&top, &["rev-parse", "HEAD"])?;
        let base = in_worktree(&worktree, &["merge-base", "HEAD", head.trim()])?;
        in_worktree(&worktree, &["reset", "-q", "--hard", base.trim()])?;
        in_worktree(&worktree, &["clean", "-q", "-fd"])?;
        Ok(())
    }

    /// Tidies task worktrees in the background as the app starts: git forgets the worktrees
    /// whose folders are gone from each project's repository, and Patchwork the records of
    /// worktrees whose folder is gone. A worktree still on disk is kept, with the work in it.
    pub fn start_worktree_cleanup(self: &Arc<Self>) {
        if !self.store.owns_data() {
            return;
        }
        let core = Arc::clone(self);
        std::thread::spawn(move || core.clean_worktrees());
    }

    fn clean_worktrees(&self) {
        for project in self.store.settings().projects {
            if ssh::is_remote(&project.path) || !Path::new(&project.path).is_dir() {
                continue;
            }
            if git(&project.path, &["rev-parse", "--show-toplevel"]).is_ok()
                && let Err(e) = git(&project.path, &["worktree", "prune"])
            {
                log::warn!(error:% = e; "could not prune a project's worktrees: {{error}}");
            }
        }
        for record in self.store.state_keys("worktree:") {
            let Some(stored) = self
                .store
                .state(&record)
                .and_then(|v| serde_json::from_str::<Worktree>(&v).ok())
            else {
                continue;
            };
            if Path::new(&stored.top).exists() {
                continue;
            }
            match self.store.clear_state(&record) {
                Ok(()) => {
                    log::info!(branch:% = stored.branch; "forgot the worktree on {{branch}}, whose folder is gone");
                }
                Err(e) => log::warn!(error:% = e; "could not forget a missing worktree: {{error}}"),
            }
        }
    }

    /// Removes task `thread_id`'s worktree as the task goes. What it hadn't committed is committed
    /// to its branch first, and the branch stays unless the project already has all of it.
    pub(crate) fn remove_thread_worktree(&self, thread_id: &str) {
        let Some(worktree) = self.thread_worktree(thread_id) else {
            return;
        };
        let removed = (|| -> Result<(), Error> {
            // Its agent may have left it on another branch; what it holds goes to its own.
            self.switch_back(thread_id, &worktree)?;
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
