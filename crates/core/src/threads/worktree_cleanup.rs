//! The worktree cleanup rules of the settings, which the background cleanup runs. A rule removes
//! a task's worktree folder and keeps its branch and the task, so the task's next turn makes the
//! worktree again where it was.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::path::Path;

use crate::Core;
use crate::settings::{KeepWorktrees, WorktreeCleanup};
use crate::system::{now_ms, ssh};
use crate::threads::Thread;
use crate::workspace;

use super::worktree::{Worktree, git, in_progress, main_top, merge_lock, unshipped};

/// A day, in milliseconds.
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

impl Core {
    /// Removes the folders of the task worktrees a cleanup rule picks: once for each worktree,
    /// by the rules of its first task's project. A worktree a task is running in, or that a
    /// merge, update or other step holds, is left for the next run.
    pub(super) fn apply_cleanup_rules(&self) {
        let mut seen = HashSet::new();
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
            let Some(project) = first.project_id.as_deref() else {
                continue;
            };
            let rules = self.cleanup_rules(Some(project));
            if !(rules.merged || rules.inactive || rules.archived || rules.unchanged) {
                continue;
            }
            if let Err(e) = self.clean_by_rules(&first.id, &worktree, &threads, &rules) {
                log::warn!(branch:% = worktree.branch, error:% = e; "could not clean up the worktree on {{branch}}: {{error}}");
            }
        }
    }

    /// Removes the folder of `worktree`, which `threads` share with `owner` first, when one of
    /// `rules` picks it and its keep rule doesn't keep it.
    fn clean_by_rules(
        &self,
        owner: &str,
        worktree: &Worktree,
        threads: &[Thread],
        rules: &WorktreeCleanup,
    ) -> Result<(), crate::Error> {
        let Some(top) = main_top(&worktree.top) else {
            return Ok(());
        };
        if ssh::is_remote(&top) || !Path::new(&worktree.top).is_dir() {
            return Ok(());
        }
        // Taken before the busy check, which a turn makes after claiming its start.
        let Ok(_step) = self.hold_worktree(owner) else {
            return Ok(());
        };
        if self.busy(owner) || in_progress(&worktree.top) {
            return Ok(());
        }
        let _merging = merge_lock(&top, || {})?;
        if kept(worktree, rules.keep)? {
            return Ok(());
        }
        let reason = if rules.archived && archived_for(threads, rules.archived_days) {
            "archived"
        } else if rules.inactive && inactive_for(threads, rules.inactive_days) {
            "inactive"
        } else if rules.unchanged && self.unchanged(owner)? {
            "unchanged"
        } else if rules.merged && merged(worktree, &top)? {
            "merged"
        } else {
            return Ok(());
        };
        log::info!(branch:% = worktree.branch, reason:%; "cleaning up the {{reason}} worktree on {{branch}}");
        self.park_worktree(worktree, &top)
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

/// Whether every task in `threads` is archived, the last of them `days` ago or longer.
fn archived_for(threads: &[Thread], days: u32) -> bool {
    let last = threads
        .iter()
        .map(|t| t.archived_at)
        .try_fold(0, |last, at| at.map(|at| last.max(at)));
    last.is_some_and(|at| now_ms().saturating_sub(at) >= u64::from(days) * DAY_MS)
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
