//! Cleaning up task worktrees from the settings. Each kind removes the folders of the task
//! worktrees it picks once, and keeps their branches and tasks, so a task's next turn makes its
//! worktree again where it was.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::path::Path;

use crate::Core;
use crate::settings::KeepWorktrees;
use crate::system::{now_ms, ssh};
use crate::threads::Thread;
use crate::workspace;

use super::worktree::{Worktree, git, in_progress, main_top, merge_lock, unshipped};

/// A day, in milliseconds.
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// How long a task goes without a message before its worktree counts as inactive.
const INACTIVE_DAYS: u32 = 14;

/// Which task worktrees a cleanup removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeSweep {
    /// Those whose branch the project or a merged pull request has.
    Merged,
    /// Those whose tasks have had no message for two weeks.
    Inactive,
    /// Those whose tasks are all archived.
    Archived,
    /// Those with no commits or changes of their own.
    Unchanged,
}

impl Core {
    /// Removes the folders of the task worktrees `sweep` picks, once for each worktree, and
    /// returns how many it removed. A worktree the keep setting keeps, a task is running in, or
    /// a merge, update or other step holds, stays.
    pub fn clean_up_worktrees(&self, sweep: WorktreeSweep) -> usize {
        let keep = self.store.settings().worktrees.cleanup.keep;
        let mut seen = HashSet::new();
        let mut removed = 0;
        for record in self.store.state_keys("worktree:") {
            let Some(id) = record.strip_prefix("worktree:") else {
                continue;
            };
            let Some(worktree) = self.thread_worktree(id) else {
                continue;
            };
            if !seen.insert(worktree.top.clone()) {
                continue;
            }
            let threads: Vec<Thread> = self
                .sharers(&worktree.top)
                .iter()
                .filter_map(|id| self.store.thread(id))
                .collect();
            let Some(first) = threads.iter().min_by_key(|t| t.created_at) else {
                continue;
            };
            match self.clean_by(sweep, keep, &first.id, &worktree, &threads) {
                Ok(true) => removed += 1,
                Ok(false) => {}
                Err(e) => {
                    log::warn!(branch:% = worktree.branch, error:% = e; "could not clean up the worktree on {{branch}}: {{error}}");
                }
            }
        }
        removed
    }

    /// Removes the folder of `worktree`, which `threads` share with `owner` first, when `sweep`
    /// picks it and `keep` doesn't keep it. Returns whether it did.
    fn clean_by(
        &self,
        sweep: WorktreeSweep,
        keep: KeepWorktrees,
        owner: &str,
        worktree: &Worktree,
        threads: &[Thread],
    ) -> Result<bool, crate::Error> {
        let Some(top) = main_top(&worktree.top) else {
            return Ok(false);
        };
        if ssh::is_remote(&top) || !Path::new(&worktree.top).is_dir() {
            return Ok(false);
        }
        // Taken before the busy check, which a turn makes after claiming its start.
        let Ok(_step) = self.hold_worktree(owner) else {
            return Ok(false);
        };
        if self.busy(owner) || in_progress(&worktree.top) {
            return Ok(false);
        }
        let _merging = merge_lock(&top, || {})?;
        if kept(worktree, keep)? {
            return Ok(false);
        }
        let picked = match sweep {
            WorktreeSweep::Archived => archived(threads),
            WorktreeSweep::Inactive => inactive_for(threads, INACTIVE_DAYS),
            WorktreeSweep::Unchanged => self.unchanged(owner)?,
            WorktreeSweep::Merged => merged(worktree, &top)?,
        };
        if !picked {
            return Ok(false);
        }
        log::info!(branch:% = worktree.branch, sweep:? = sweep; "cleaning up the worktree on {{branch}}: {{sweep}}");
        self.park_worktree(worktree, &top)?;
        Ok(true)
    }

    /// Whether task `thread_id`'s worktree has neither commits nor changes of its own.
    fn unchanged(&self, thread_id: &str) -> Result<bool, crate::Error> {
        Ok(self
            .worktree_state(thread_id)?
            .is_some_and(|s| s.commits == 0 && s.changes.is_empty() && s.off_branch.is_none()))
    }
}

/// Whether `keep` keeps `worktree` as it is: with changes not committed, tracked files changed,
/// or any file, ignored ones too, that its branch doesn't hold.
fn kept(worktree: &Worktree, keep: KeepWorktrees) -> Result<bool, crate::Error> {
    let args: &[&str] = match keep {
        KeepWorktrees::Uncommitted => &["status", "--porcelain"],
        KeepWorktrees::Tracked => &["status", "--porcelain", "--untracked-files=no"],
        KeepWorktrees::AnyFiles => &["status", "--porcelain", "--ignored"],
    };
    Ok(!git(&worktree.top, args)?.trim().is_empty())
}

/// Whether every task in `threads` is archived.
fn archived(threads: &[Thread]) -> bool {
    threads.iter().all(|t| t.archived_at.is_some())
}

/// Whether no task in `threads` has had a message for `days` days.
fn inactive_for(threads: &[Thread], days: u32) -> bool {
    let last = threads
        .iter()
        .flat_map(|t| {
            t.messages
                .last()
                .map(|m| m.created_at)
                .into_iter()
                .chain([t.created_at])
        })
        .max()
        .unwrap_or(0);
    now_ms().saturating_sub(last) >= u64::from(days) * DAY_MS
}

/// Whether `worktree`'s work has reached its project, whose main folder is `top`: nothing is
/// uncommitted, and its branch has commits that the project or the remote's default branch has
/// all of, or a pull request from it was merged.
fn merged(worktree: &Worktree, top: &str) -> Result<bool, crate::Error> {
    if !git(&worktree.top, &["status", "--porcelain"])?
        .trim()
        .is_empty()
    {
        return Ok(false);
    }
    let head = git(top, &["rev-parse", "HEAD"])?;
    if unshipped(worktree, head.trim())? == 0 && has_own_commits(worktree) {
        return Ok(true);
    }
    Ok(workspace::gh(
        &worktree.root,
        &["pr", "view", "--json", "state", "-q", ".state"],
    )
    .is_ok_and(|state| state.trim() == "MERGED"))
}

/// Whether `worktree`'s branch moved since it was made, by its reflog. A branch without one
/// counts as not moved.
fn has_own_commits(worktree: &Worktree) -> bool {
    let full = format!("refs/heads/{}", worktree.branch);
    let Ok(log) = git(
        &worktree.top,
        &["reflog", "show", "--format=%H", &full, "--"],
    ) else {
        return false;
    };
    let mut hashes = log.lines();
    let (Some(now), Some(made)) = (hashes.next(), hashes.last()) else {
        return false;
    };
    now != made
}
