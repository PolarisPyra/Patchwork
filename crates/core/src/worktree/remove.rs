//! Removing a task's worktree: with its task, by the cleanup rules, or when it is no longer
//! Patchwork's, and sweeping folders no record names.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::system::ssh;
use crate::{Core, Error};
use super::condition::{Condition, repaired_condition};
use super::create::build_dir;
use super::git::{canonical, git, in_progress, in_worktree, locked, main_top, off_branch};
use super::landed::{delete_if_landed, unshipped};
use super::state::{Worktree, key};

/// How old a folder in the worktrees folder that no record names must be before it is swept.
const SWEEP_AFTER: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// How long the background cleanup waits between runs.
const CLEANUP_EVERY: std::time::Duration = std::time::Duration::from_secs(60 * 60);

impl Core {
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

    /// Resets task `chat_id`'s worktree to the branch that already has every file it changed,
    /// once it has no commits of its own, so it stops showing changes the project has. Returns
    /// that branch.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has no worktree or is still running, when the worktree has
    /// commits or changes the project doesn't, or when git fails.
    pub fn clean_up_worktree(&self, chat_id: &str) -> Result<String, Error> {
        let worktree = self
            .chat_worktree(chat_id)
            .ok_or("This task doesn't have a worktree")?;
        let _step = self.begin_step(
            chat_id,
            "Wait for the task and its background commands to finish before cleaning it up",
        )?;
        let top = self.verify(chat_id, &worktree)?;
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
                let chat_id = record.strip_prefix("worktree:").unwrap_or(&record);
                let found = (!stored.parked && self.store.chat(chat_id).is_some())
                    .then(|| self.project_top(chat_id).ok())
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
            let chat_id = record.strip_prefix("worktree:").unwrap_or(&record);
            // A cleanup rule removed its folder; its task makes it again, so the record stays.
            if stored.parked
                && (self.store.chat(chat_id).is_some()
                    || self.shared_with_others(&stored.top, chat_id))
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
                    let Ok(_step) = self.hold_worktree(chat_id) else {
                        continue;
                    };
                    if self.busy(chat_id)
                        || self.agent_up(chat_id)
                        || self.worktree_record(chat_id).as_ref() != Some(&stored)
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
                None if self.store.chat(chat_id).is_some() => {
                    if !Path::new(&stored.top).exists() {
                        continue;
                    }
                }
                Some(Condition::Present | Condition::Parked) | None => {}
            }
            if let Ok(top) = std::fs::canonicalize(&stored.top) {
                recorded.push(top);
                if self.store.chat(chat_id).is_none() {
                    self.remove_orphan(chat_id, &stored);
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
            self.sweep_unrecorded(&super::home::home(top), &recorded);
        }
        self.settle_landed_worktrees();
    }

    /// Removes task `chat_id`'s worktree as the task goes. What it hadn't committed is committed
    /// to its branch first, and the branch stays unless the project, or the remote's default
    /// branch, already has all of it.
    ///
    /// The worktree stays while another task shares it, and when the project keeps the worktrees
    /// of deleted tasks; the record stays with it then.
    pub(crate) fn remove_chat_worktree(&self, chat_id: &str) {
        let Some(record) = self.worktree_record(chat_id) else {
            return;
        };
        if self.shared_with_others(&record.top, chat_id) {
            self.forget_worktree(chat_id);
            return;
        }
        if !self.removes_with_tasks() {
            return;
        }
        if record.parked {
            // Only its branch is left, which goes once the project has all of it.
            if let Ok(top) = self.project_top(chat_id)
                && let Err(e) = delete_if_landed(&top, &record.branch)
            {
                log::warn!(error:% = e; "could not delete a deleted task's branch: {{error}}");
            }
            self.forget_worktree(chat_id);
            return;
        }
        let Some(worktree) = self.chat_worktree(chat_id) else {
            return;
        };
        let removed = (|| -> Result<(), Error> {
            let _step = self.hold_worktree(chat_id)?;
            // Committing in the middle of a merge would record its conflicts; the worktree stays.
            if in_progress(&worktree.top) {
                return Err("a merge, rebase or cherry-pick is in progress in it".into());
            }
            // Its agent may have left it on another branch; what it holds goes to its own.
            self.switch_back(chat_id, &worktree)?;
            let top = self.verify(chat_id, &worktree)?;
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

    /// Forgets task `chat_id`'s record of its worktree.
    pub(super) fn forget_worktree(&self, chat_id: &str) {
        if let Err(e) = self.store.clear_state(&key(chat_id)) {
            log::warn!(error:% = e; "could not forget a task's worktree: {{error}}");
        }
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

    /// Removes the worktree of task `chat_id`, which was deleted while its worktree couldn't be
    /// removed, and forgets it. A worktree it can't remove yet stays, with its record, and so
    /// does one another task shares or its project keeps.
    fn remove_orphan(&self, chat_id: &str, worktree: &Worktree) {
        if self.shared_with_others(&worktree.top, chat_id) {
            self.forget_worktree(chat_id);
            return;
        }
        if !self.owns(worktree) || !self.removes_with_tasks() {
            return;
        }
        let removed = (|| -> Result<(), Error> {
            let _step = self.hold_worktree(chat_id)?;
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
