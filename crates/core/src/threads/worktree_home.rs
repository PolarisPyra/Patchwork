//! Where worktrees live: a `.patchwork` folder in the project's repository, kept out of what git
//! lists, and moving there the ones an earlier version made in the app's data folder.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};

use crate::system::ssh;
use crate::{Core, Error};

use super::worktree::{Worktree, git, key, locked};
use super::worktree_condition::{Condition, repaired_condition};

/// The project's folder for what Patchwork keeps in it.
const PROJECT_DIR: &str = ".patchwork";

/// The folder in [`PROJECT_DIR`] that holds worktrees. A development build keeps its own, so it
/// never sweeps the installed app's.
const FOLDER: &str = if cfg!(debug_assertions) {
    "worktrees-dev"
} else {
    "worktrees"
};

/// The folder the worktrees of the repository at `top` go in.
pub(crate) fn home(top: &str) -> PathBuf {
    Path::new(top).join(PROJECT_DIR).join(FOLDER)
}

/// Whether `dir` is a repository's worktrees folder, by where it is.
pub(crate) fn is_home(dir: &Path) -> bool {
    let named = |path: &Path, name: &str| path.file_name().is_some_and(|n| n == name);
    named(dir, FOLDER) && dir.parent().is_some_and(|p| named(p, PROJECT_DIR))
}

/// The top folder of the worktree `path` is in, when it is in a repository's worktrees folder.
pub(crate) fn top_of(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|at| at.parent().is_some_and(is_home))
        .map(Path::to_path_buf)
}

/// Makes the worktrees folder of the repository at `top`, and keeps it out of `git status`
/// through the repository's own exclude file, which no commit carries.
///
/// # Errors
///
/// Returns an error when the folder cannot be made. A failure to write the exclude file is
/// logged, and leaves the folder listed as untracked.
pub(crate) fn make_home(top: &str) -> Result<PathBuf, Error> {
    let dir = home(top);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    if let Err(e) = exclude(top) {
        log::warn!(error:% = e; "could not keep the worktrees folder out of git status: {{error}}");
    }
    Ok(dir)
}

/// Adds the worktrees folder to `info/exclude` of the repository at `top`, once.
fn exclude(top: &str) -> Result<(), Error> {
    let common = git(
        top,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let info = Path::new(common.trim()).join("info");
    let file = info.join("exclude");
    let line = format!("/{PROJECT_DIR}/{FOLDER}/");
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    if text.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    std::fs::create_dir_all(&info)?;
    let gap = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    std::fs::write(&file, format!("{text}{gap}{line}\n"))?;
    Ok(())
}

/// Moves the worktree at `from` of the repository at `repo` into the repository's worktrees
/// folder, under the same name, and returns where it is now.
fn move_worktree(repo: &str, from: &Path) -> Result<PathBuf, Error> {
    let name = from.file_name().ok_or("the worktree has no folder name")?;
    let to = make_home(repo)?.join(name);
    if to.exists() {
        return Err(format!("{} is already there", to.display()).into());
    }
    git(
        repo,
        &[
            "worktree",
            "move",
            "--",
            &from.to_string_lossy(),
            &to.to_string_lossy(),
        ],
    )?;
    Ok(to)
}

/// `path`, which is at or under `from`, at the same place under `to`.
fn rebased(path: &str, from: &Path, to: &Path) -> String {
    let rest = Path::new(path).strip_prefix(from).unwrap_or(Path::new(""));
    to.join(rest)
        .to_string_lossy()
        .trim_end_matches('/')
        .to_owned()
}

impl Core {
    /// Whether `dir` is the worktrees folder an earlier version kept in the app's data folder.
    pub(super) fn is_old_home(&self, dir: &Path) -> bool {
        std::fs::canonicalize(&self.worktrees_dir).is_ok_and(|old| old == dir)
            || dir == self.worktrees_dir
    }

    /// The top folder of the worktree `path` is in, wherever Patchwork keeps worktrees.
    pub(crate) fn worktree_top_of(&self, path: &Path) -> Option<PathBuf> {
        top_of(path).or_else(|| {
            let old = std::fs::canonicalize(&self.worktrees_dir).ok()?;
            let folder = path.strip_prefix(&old).ok()?.components().next()?;
            Some(old.join(folder))
        })
    }

    /// Moves the worktrees still in the app's data folder into their projects' own folders: each
    /// task's, with its record, and each project that is a worktree. One in use, locked, or that
    /// git refuses to move stays where it is, and works from there until a later pass moves it.
    pub(super) fn move_old_worktrees(&self) {
        let records: Vec<(String, Worktree)> = self
            .store
            .state_keys("worktree:")
            .into_iter()
            .filter_map(|record| {
                let stored = serde_json::from_str::<Worktree>(&self.store.state(&record)?).ok()?;
                let id = record.strip_prefix("worktree:")?.to_owned();
                let at_old = Path::new(&stored.top)
                    .parent()
                    .is_some_and(|p| self.is_old_home(p));
                at_old.then_some((id, stored))
            })
            .collect();
        for (thread_id, stored) in records {
            // A fork sharing it moved with the task before it.
            if self.worktree_record(&thread_id).as_ref() != Some(&stored) {
                continue;
            }
            match self.move_task_worktree(&thread_id, &stored) {
                Ok(Some(to)) => {
                    log::info!(branch:% = stored.branch, to:%; "moved the worktree on {{branch}} to {{to}}");
                }
                Ok(None) => {}
                Err(e) => {
                    log::warn!(branch:% = stored.branch, error:% = e; "could not move the worktree on {{branch}} into its project: {{error}}");
                }
            }
        }
        for project in self.store.settings().projects {
            if ssh::is_remote(&project.path) {
                continue;
            }
            let path = Path::new(&project.path);
            let Some(top) = path
                .ancestors()
                .find(|at| at.parent().is_some_and(|p| self.is_old_home(p)))
            else {
                continue;
            };
            if let Err(e) = self.move_project_worktree(&project.id, path, top) {
                log::warn!(project:% = project.name, error:% = e; "could not move the worktree of {{project}} into its repository: {{error}}");
            }
        }
    }

    /// Moves task `thread_id`'s worktree `stored`, and every sharing task's record of it. None
    /// when it waits for a later pass: its task is gone or busy, or its folder isn't as recorded.
    fn move_task_worktree(
        &self,
        thread_id: &str,
        stored: &Worktree,
    ) -> Result<Option<String>, Error> {
        if self.store.thread(thread_id).is_none() {
            return Ok(None);
        }
        let Ok(_step) = self.hold_worktree(thread_id) else {
            return Ok(None);
        };
        if self.busy(thread_id) || self.agent_up(thread_id) {
            return Ok(None);
        }
        let repo = self.project_top(thread_id)?;
        let from = Path::new(&stored.top);
        let to = if stored.parked {
            // Its folder is gone; the task's next turn makes it again at the new place.
            let name = from.file_name().ok_or("the worktree has no folder name")?;
            home(&repo).join(name)
        } else {
            if repaired_condition(stored, &repo) != Condition::Present || locked(&stored.top) {
                return Ok(None);
            }
            move_worktree(&repo, from)?
        };
        let moved = Worktree {
            root: rebased(&stored.root, from, &to),
            top: to.to_string_lossy().into_owned(),
            ..stored.clone()
        };
        let record = serde_json::to_string(&moved).map_err(|e| e.to_string())?;
        for id in self.sharers(&stored.top) {
            self.store.set_state(&key(&id), &record)?;
            // Agents keep their sessions by folder, so the next message starts a new one, sent
            // the transcript.
            self.store.update_thread(&id, |t| {
                if t.session_id.take().is_some() {
                    t.context_pending = !t.messages.is_empty();
                }
            })?;
        }
        Ok(Some(moved.top))
    }

    /// Moves the worktree at `top` that project `id`, at `path` in it, is, and points the project
    /// at where it is now. Waits for a later pass while one of the project's tasks runs.
    fn move_project_worktree(&self, id: &str, path: &Path, top: &Path) -> Result<(), Error> {
        let running = self.store.threads().into_iter().any(|t| {
            t.project_id.as_deref() == Some(id)
                && (self.runner.is_running(&t.id) || self.runner.has_agent(&t.id))
        });
        let from = top.to_string_lossy();
        if running || !top.is_dir() || locked(&from) {
            return Ok(());
        }
        let Some(repo) = super::worktree::main_top(&from) else {
            return Ok(());
        };
        let to = move_worktree(&repo, top)?;
        self.relocate_project(id, &rebased(&path.to_string_lossy(), top, &to))
    }
}
