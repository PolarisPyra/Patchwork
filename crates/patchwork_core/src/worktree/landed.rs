//! Whether a task branch's work reached the project, and the marks the sidebar shows for it.

// Rust guideline compliant 2026-10-09

use std::path::Path;
use std::collections::{HashMap, HashSet};
use crate::system::ssh;
use crate::chats::Role;
use crate::source_control::{self, FileChange, landed};
use crate::{Core, Error};
use super::git::{git, off_branch};
use super::state::{Worktree, WorktreeMark, failed_key};

/// Deletes `branch` from the repository at `top` when it is there and the default branch
/// already has all of it, even under other commits, as a squash-merged pull request leaves it.
/// Whatever branch the project's folder has checked out doesn't count. Returns whether it was
/// deleted.
pub(super) fn delete_if_landed(top: &str, branch: &str) -> Result<bool, Error> {
    let full = format!("refs/heads/{branch}");
    if git(top, &["rev-parse", "--verify", "-q", &full]).is_err() {
        return Ok(false);
    }
    let landed = source_control::base_ref(top).is_some_and(|base| landed(top, &base, &full));
    if !landed {
        return Ok(false);
    }
    git(top, &["branch", "-q", "-D", "--", branch])?;
    Ok(true)
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
    source_control::fetch_remote(repo);
    let base = source_control::base_ref(repo);
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
    base.is_some_and(|base| landed(repo, base, rev)) || source_control::pr_merged(repo, branch, rev)
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

impl Core {
    /// The branch, the project's or the remote's default, that has every file `worktree`
    /// changed as it has them, with the commit to reset to. `head` is the project's HEAD.
    pub(super) fn landed_on(
        worktree: &Worktree,
        project: &str,
        head: &str,
    ) -> Result<Option<(String, String)>, Error> {
        // Every change in the worktree, also those outside the project's folder in it.
        let changes = source_control::git_changes(&worktree.top)?;
        let mut candidates = vec![(
            source_control::git_branch(project).unwrap_or_else(|| "the project's HEAD".into()),
            head.to_owned(),
        )];
        if let Some(base) = source_control::base_ref(&worktree.top)
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
                self.chat_worktree(id)
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
        for chat in self.list_chats() {
            let Some(project_id) = chat.project_id.as_deref() else {
                continue;
            };
            if marks.contains_key(&chat.id)
                || chat.archived_at.is_some()
                || chat.parent.is_some()
                || chat.session_id.is_some()
                || chat.messages.iter().any(|m| m.role == Role::Assistant)
                || self.worktree_record(&chat.id).is_some()
                || self.store.state(&failed_key(&chat.id)).is_some()
                || self.edits_in_place(project_id)
                || self.is_dispatcher(&chat.id)
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
                marks.insert(chat.id.clone(), WorktreeMark::Clean);
            }
        }
    }
}
