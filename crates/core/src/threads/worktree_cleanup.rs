//! Deleting stale task worktrees from the settings: removes the folder of every task worktree
//! no task or terminal is working in, and keeps its branch and tasks, so a task's next turn makes its
//! worktree again where it was. A worktree whose tasks were all deleted goes with its record, and
//! its branch too once the project has all of it.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::path::Path;

use crate::Core;
use crate::settings::KeepWorktrees;
use crate::system::{processes, ssh};
use crate::workspace;

use super::worktree::{Worktree, git, in_progress, locked, main_top, merge_lock, off_branch};
use super::worktree_condition::{Condition, condition};

impl Core {
    /// Removes the folder of every stale task worktree, once for each worktree, and returns how
    /// many it removed. A worktree the keep setting keeps, a task is running in, or a merge,
    /// update or other step holds, stays.
    pub fn delete_stale_worktrees(&self, open: Option<&str>) -> usize {
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
            let sharers = self.sharers(&worktree.top);
            if open.is_some_and(|open| sharers.iter().any(|id| id == open)) {
                continue;
            }
            // A worktree whose tasks were all deleted, kept as the project keeps those, is held
            // by its record's id, which names the same folder.
            let owner = sharers
                .iter()
                .filter_map(|id| self.store.thread(id))
                .min_by_key(|t| t.created_at)
                .map_or_else(|| id.to_owned(), |t| t.id);
            match self.delete_if_stale(keep, &owner, &worktree) {
                Ok(true) => removed += 1,
                Ok(false) => {}
                Err(e) => {
                    log::warn!(branch:% = worktree.branch, error:% = e; "could not delete the worktree on {{branch}}: {{error}}");
                }
            }
        }
        removed
    }

    /// Removes the folder of `worktree`, which `owner` made, unless its link to its repository
    /// broke, a task or its agent is in it, a step holds it, it is locked or off its branch, or
    /// `keep` keeps it. Returns whether it did. When `owner` was deleted, the worktree goes with
    /// every record of it.
    fn delete_if_stale(
        &self,
        keep: KeepWorktrees,
        owner: &str,
        worktree: &Worktree,
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
        if condition(worktree, &top) != Condition::Present
            || self.busy(owner)
            || self.agent_up(owner)
            || in_progress(&worktree.top)
            || locked(&worktree.top)
            // Committing to another branch, or a detached HEAD the folder takes with it, would
            // leave the work where its record doesn't find it.
            || off_branch(worktree).is_some()
            || self.worked_in(&worktree.top)
        {
            return Ok(false);
        }
        let _merging = merge_lock(&top, || {})?;
        if kept(worktree, keep)? {
            return Ok(false);
        }
        log::info!(branch:% = worktree.branch; "deleting the stale worktree on {{branch}}");
        if self.store.thread(owner).is_some() {
            self.park_worktree(worktree, &top)?;
        } else {
            Self::drop_worktree(worktree, &top)?;
            for id in self.sharers(&worktree.top) {
                self.forget_worktree(&id);
            }
        }
        Ok(true)
    }

    /// Whether a process other than Patchwork, its agents, and the git and `gh` commands it is
    /// running is working in `top`: a terminal's shell, a command an agent left running, an
    /// editor, or a shell elsewhere. Where the system can't tell, whether a terminal's shell is.
    pub(super) fn worked_in(&self, top: &str) -> bool {
        let Some(pids) = processes::working_in(Path::new(top)) else {
            return self
                .terminals
                .shells()
                .iter()
                .any(|(_, cwd)| Path::new(cwd).starts_with(top));
        };
        let mut ignored: HashSet<i32> = self
            .runner
            .agent_pids()
            .into_iter()
            .filter_map(|pid| i32::try_from(pid).ok())
            .collect();
        ignored.insert(std::process::id() as i32);
        let commands = workspace::own_commands();
        if !commands.is_empty()
            && let Some(table) = processes::Snapshot::read()
        {
            ignored.extend(
                commands
                    .iter()
                    .filter_map(|pid| i32::try_from(*pid).ok())
                    .flat_map(|pid| table.tree(pid))
                    .map(|p| p.pid),
            );
        }
        pids.iter().any(|pid| !ignored.contains(pid))
    }
}

/// Whether `keep` keeps `worktree` as it is: with changes not committed, tracked files changed,
/// or any file, ignored ones too, that its branch doesn't hold. Keeping uncommitted changes also
/// keeps ignored files the project's folder doesn't have as they are, such as an edited `.env`
/// or a local database, which committing before the folder goes would leave out.
fn kept(worktree: &Worktree, keep: KeepWorktrees) -> Result<bool, crate::Error> {
    let args: &[&str] = match keep {
        KeepWorktrees::Uncommitted => &["status", "--porcelain"],
        KeepWorktrees::Tracked => &["status", "--porcelain", "--untracked-files=no"],
        KeepWorktrees::AnyFiles => &["status", "--porcelain", "--ignored"],
    };
    if !git(&worktree.top, args)?.trim().is_empty() {
        return Ok(true);
    }
    Ok(keep == KeepWorktrees::Uncommitted && !ignored_only_here(&worktree.top)?.is_empty())
}

/// The ignored files of the worktree at `top` that its repository's main folder doesn't have as
/// they are, by path from `top`: files a task made or changed that git doesn't track, such as a
/// `.env` or a database. A folder git ignores whole, such as a build folder, isn't one; nor is a
/// file copied in as the main folder has it. Empty for a worktree on an SSH host.
pub(crate) fn ignored_only_here(top: &str) -> Result<Vec<String>, crate::Error> {
    if ssh::is_remote(top) {
        return Ok(Vec::new());
    }
    let Some(main) = main_top(top) else {
        return Ok(Vec::new());
    };
    let listed = git(top, &["status", "--porcelain", "--ignored", "-z"])?;
    Ok(listed
        .split('\0')
        .filter_map(|entry| entry.strip_prefix("!! "))
        .filter(|path| !path.ends_with('/'))
        .filter(|path| {
            let here = std::fs::read(Path::new(top).join(path));
            let there = std::fs::read(Path::new(&main).join(path));
            !matches!((here, there), (Ok(a), Ok(b)) if a == b)
        })
        .map(str::to_owned)
        .collect())
}
