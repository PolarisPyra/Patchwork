//! Settling landed task branches: once a task branch's work is on the default branch, under
//! other commits as a squash-merge leaves it or as a merged pull request, the branch moves to
//! the default branch, so it has nothing of its own and stops showing as unmerged however the
//! default branch changes those lines later.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;

use crate::system::ssh;
use crate::source_control;
use crate::{Core, Error};

use super::git::{checked_out_at, git, in_progress, in_worktree, merge_lock, off_branch};
use super::landed::landed_branch;
use super::state::Worktree;
use super::condition::{Condition, condition};

impl Core {
    /// Moves the branch of every task worktree whose work has landed to the default branch, once
    /// for each worktree. A worktree a task or a terminal is working in, a step holds, or with
    /// anything not committed stays as it is.
    pub(super) fn settle_landed_worktrees(&self) {
        let mut seen = HashSet::new();
        for record in self.store.state_keys("worktree:") {
            let Some(id) = record.strip_prefix("worktree:") else {
                continue;
            };
            let worktree = match self.worktree_record(id) {
                Some(stored) if stored.parked => stored,
                Some(_) => match self.chat_worktree(id) {
                    Some(worktree) => worktree,
                    None => continue,
                },
                None => continue,
            };
            if !worktree.branch.starts_with("patchwork/") || !seen.insert(worktree.top.clone()) {
                continue;
            }
            let Some(owner) = self.mark_owner(&worktree.top) else {
                continue;
            };
            match self.settle(&owner, &worktree) {
                Ok(Some(base)) => {
                    log::info!(branch:% = worktree.branch, base:%; "moved {{branch}}, which has landed, to {{base}}");
                }
                Ok(None) => {}
                Err(e) => {
                    log::warn!(branch:% = worktree.branch, error:% = e; "could not settle the landed branch {{branch}}: {{error}}");
                }
            }
        }
    }

    /// Moves `worktree`'s branch, which `owner` made, to the default branch when it has landed
    /// there and nothing works in it. Returns the default branch it moved to, if it did.
    fn settle(&self, owner: &str, worktree: &Worktree) -> Result<Option<String>, Error> {
        let repo = self.project_top(owner)?;
        if ssh::is_remote(&repo) {
            return Ok(None);
        }
        // Taken before the busy check, which a turn makes after claiming its start.
        let Ok(_step) = self.hold_worktree(owner) else {
            return Ok(None);
        };
        if self.busy(owner) {
            return Ok(None);
        }
        let _merging = merge_lock(&repo, || {})?;
        source_control::fetch_remote(&repo);
        let Some(base) = source_control::base_ref(&repo) else {
            return Ok(None);
        };
        let branch = format!("refs/heads/{}", worktree.branch);
        let tip = git(&repo, &["rev-parse", "--verify", "-q", &branch])?;
        let target = git(
            &repo,
            &["rev-parse", "--verify", "-q", &format!("{base}^{{commit}}")],
        )?;
        let (tip, target) = (tip.trim(), target.trim());
        let own: usize = git(&repo, &["rev-list", "--count", tip, "--not", target])?
            .trim()
            .parse()
            .unwrap_or(0);
        if own == 0 || !landed_branch(&repo, tip, &worktree.branch, Some(&base)) {
            return Ok(None);
        }
        if worktree.parked {
            // `update-ref` moves a branch even under a worktree that has it checked out, which
            // `worktree add` would refuse; a branch someone checked out stays as it is.
            if checked_out_at(&repo, &worktree.branch)?.is_some() {
                return Ok(None);
            }
            // Moves it only while it is still at `tip`.
            git(&repo, &["update-ref", &branch, target, tip])?;
            return Ok(Some(base));
        }
        if condition(worktree, &repo) != Condition::Present
            || in_progress(&worktree.top)
            || off_branch(worktree).is_some()
            || self.agent_up(owner)
            || self.worked_in(&worktree.top)
            || !git(&worktree.top, &["status", "--porcelain"])?
                .trim()
                .is_empty()
        {
            return Ok(None);
        }
        // A commit made in it since `tip` was read, by an editor or a git tool the checks above
        // don't see, isn't in `target`, and moving the branch would drop it.
        if git(&worktree.top, &["rev-parse", "--verify", "-q", "HEAD"])?.trim() != tip {
            return Ok(None);
        }
        in_worktree(worktree, &["reset", "-q", "--keep", target])?;
        Ok(Some(base))
    }
}
