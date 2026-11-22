//! A task's worktree as stored, and what the app reads of it: the folder a task works in, the
//! worktree's branch and its state against the project.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};
use crate::chats::NOT_FOUND;
use crate::source_control::{self, FileChange};
use crate::{Core, Error};
use super::home::is_home;
use super::create::build_dir;
use super::git::{NOT_OURS, git, off_branch};
use super::landed::unshipped;

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
    /// The titles of the other tasks working in the same worktree, forks that share it, whose
    /// work discarding throws away too.
    pub shared_with: Vec<String>,
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

/// The `app_state` key of why a task's worktree could not be made.
pub(super) fn failed_key(chat_id: &str) -> String {
    format!("worktree-failed:{chat_id}")
}

/// The `app_state` key a task's worktree is stored under.
pub(in crate::worktree) fn key(chat_id: &str) -> String {
    format!("worktree:{chat_id}")
}

/// The `app_state` key that gives a fork's first turn the worktree note, although it carries
/// on its source's session.
pub(super) fn brief_key(chat_id: &str) -> String {
    format!("worktree-brief:{chat_id}")
}

impl Core {
    /// The worktree task `chat_id` works in, while it is one Patchwork made and its folder is
    /// there.
    #[must_use]
    pub fn chat_worktree(&self, chat_id: &str) -> Option<Worktree> {
        let stored = self.worktree_record(chat_id)?;
        (!stored.parked && self.owns(&stored)).then_some(stored)
    }

    /// Task `chat_id`'s worktree as stored, whether or not its folder is there.
    pub(super) fn worktree_record(&self, chat_id: &str) -> Option<Worktree> {
        serde_json::from_str(&self.store.state(&key(chat_id))?).ok()
    }

    /// Why task `chat_id` works in its project's own folder although it was to get a worktree:
    /// the error making it gave.
    #[must_use]
    pub fn worktree_failure(&self, chat_id: &str) -> Option<String> {
        self.store.state(&failed_key(chat_id))
    }

    /// Whether `worktree` is in a worktrees folder of Patchwork's, a project's or the one in the
    /// app's data folder, and its root inside it, so a value changed in the database can't point
    /// a removal or a reset somewhere else.
    pub(super) fn owns(&self, worktree: &Worktree) -> bool {
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

    /// Sets whether the project's new tasks edit its own folder rather than a worktree each, as
    /// [`Settings::set_edit_in_place`](crate::settings::Settings::set_edit_in_place) does.
    /// Tasks that already have a worktree keep it.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be saved.
    pub fn set_edits_in_place(&self, project_id: &str, in_place: bool) -> Result<(), Error> {
        self.update_settings(|s| s.set_edit_in_place(project_id, in_place))
    }

    /// Whether `path` is a worktree Patchwork made as a project of its own, which needs no
    /// second worktree.
    pub(super) fn is_worktree_project(&self, path: &str) -> bool {
        std::fs::canonicalize(path).is_ok_and(|path| self.worktree_top_of(&path).is_some())
    }

    /// The folder task `chat_id` works in: its worktree, its project's folder, or its chat's
    /// own folder.
    ///
    /// # Errors
    ///
    /// Returns an error when the task or its project is unknown, and when the task has a
    /// worktree that isn't there now: its work is on its branch, so the project's own folder
    /// doesn't stand in for it, where restoring a checkpoint or discarding a file would change
    /// the user's checkout.
    pub fn chat_root(&self, chat_id: &str) -> Result<String, Error> {
        let chat = self.store.chat(chat_id).ok_or(NOT_FOUND)?;
        let owner = chat.parent.as_ref().map_or(chat_id, |p| p.chat_id.as_str());
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
        match &chat.project_id {
            Some(project) => self.project_root(project),
            None => Ok(self.chat_dir(chat_id).to_string_lossy().into_owned()),
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

    /// What task `chat_id`'s worktree holds that its project doesn't have yet. None for a task
    /// without one.
    ///
    /// # Errors
    ///
    /// Returns an error when git can't read the worktree or the project.
    pub fn worktree_state(&self, chat_id: &str) -> Result<Option<WorktreeState>, Error> {
        let Some(worktree) = self.chat_worktree(chat_id) else {
            return Ok(None);
        };
        let changes = source_control::git_changes(&worktree.root)?;
        let project = self.project_top(chat_id)?;
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
        let shared_with = self
            .sharers(&worktree.top)
            .into_iter()
            .filter(|id| id != chat_id)
            .filter_map(|id| self.store.chat(&id))
            .map(|t| t.title)
            .collect();
        Ok(Some(WorktreeState {
            worktree,
            changes,
            commits,
            behind,
            off_branch,
            landed_on,
            shared_with,
        }))
    }

    /// Everything task `chat_id`'s worktree changed since its branch left the project's
    /// branch, commits included. None for a task without one.
    ///
    /// # Errors
    ///
    /// Returns an error when git can't read the worktree or the project.
    pub fn branch_compare(&self, chat_id: &str) -> Result<Option<BranchCompare>, Error> {
        let Some(worktree) = self.chat_worktree(chat_id) else {
            return Ok(None);
        };
        let project = self.project_top(chat_id)?;
        let head = git(&project, &["rev-parse", "HEAD"])?;
        let since = git(&worktree.top, &["merge-base", "HEAD", head.trim()])?
            .trim()
            .to_owned();
        let base =
            source_control::git_branch(&project).unwrap_or_else(|| "the project's HEAD".into());
        let commits = git(
            &worktree.top,
            &["rev-list", "--count", &format!("{since}..HEAD")],
        )?
        .trim()
        .parse()
        .unwrap_or(0);
        let files = source_control::changes_since(&worktree.root, &since)?;
        Ok(Some(BranchCompare {
            root: worktree.root,
            base,
            since,
            commits,
            files,
        }))
    }

    /// The top folder of the repository task `chat_id`'s project is in.
    pub(super) fn project_top(&self, chat_id: &str) -> Result<String, Error> {
        let chat = self.store.chat(chat_id).ok_or(NOT_FOUND)?;
        let project = self.project_root(chat.project_id.as_deref().ok_or(NOT_FOUND)?)?;
        Ok(git(&project, &["rev-parse", "--show-toplevel"])?
            .trim()
            .to_owned())
    }
}
