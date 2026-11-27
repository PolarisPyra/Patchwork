//! The steps the user takes on a worktree, one at a time: merging it, updating it from the
//! project, discarding its changes, and switching it back to its branch.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::fs::File;
use crate::system::ssh;
use crate::chats::{NOT_FOUND, Chat};
use crate::source_control::{self};
use crate::{Core, Error};
use super::git::{git, identity, in_progress, in_worktree, merge_lock, off_branch};
use super::state::Worktree;

/// A hold on a worktree while a merge, update, discard, switch or removal works on it, shared
/// by every task working there. It lets go as it drops.
pub(crate) struct Step<'a> {
    core: &'a Core,
    /// The worktree's top folder, or the task's id when it has none.
    held: String,
}

impl Drop for Step<'_> {
    fn drop(&mut self) {
        self.core.worktree_steps.locked().remove(&self.held);
    }
}

/// Source Control's hold on the folder it commits or pulls in. It lets go as it drops.
pub(crate) struct SourceHold<'a> {
    _step: Option<Step<'a>>,
    _merging: Option<File>,
}

impl Core {
    /// Switches task `chat_id`'s worktree back to its own branch, after its agent switched it
    /// to another. What isn't committed comes along; git refuses when that would be lost.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has no worktree or is still running, when a merge, rebase
    /// or cherry-pick is in progress in the worktree, or when git refuses the switch.
    pub fn restore_worktree_branch(&self, chat_id: &str) -> Result<(), Error> {
        let worktree = self
            .chat_worktree(chat_id)
            .ok_or("This task doesn't have a worktree")?;
        let _step = self.begin_step(
            chat_id,
            "Stop the task and its background commands before switching its branch",
        )?;
        self.switch_back(chat_id, &worktree)
    }

    /// Checks out `worktree`'s own branch, when it is on another.
    pub(super) fn switch_back(&self, chat_id: &str, worktree: &Worktree) -> Result<(), Error> {
        self.verify_repository(chat_id, worktree)?;
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

    /// Holds task `chat_id`'s worktree for one step, refused with `while_busy` when the task or
    /// one of its subagents is running or starting. The hold is taken before the check, and a
    /// turn checks for it after claiming its start, so the two can't both go ahead.
    pub(super) fn begin_step(&self, chat_id: &str, while_busy: &str) -> Result<Step<'_>, Error> {
        let step = self.hold_worktree(chat_id)?;
        if self.busy(chat_id) {
            return Err(while_busy.to_owned().into());
        }
        Ok(step)
    }

    /// What a step on task `chat_id`'s worktree holds: the worktree's folder, which the forks
    /// sharing it hold too, or the task's id when it has none.
    fn hold_key(&self, chat_id: &str) -> String {
        self.worktree_record(chat_id)
            .map_or_else(|| chat_id.to_owned(), |w| w.top)
    }

    /// Holds task `chat_id`'s worktree, refused while another step holds it.
    pub(super) fn hold_worktree(&self, chat_id: &str) -> Result<Step<'_>, Error> {
        let held = self.hold_key(chat_id);
        let fresh = self.worktree_steps.locked().insert(held.clone());
        if !fresh {
            return Err("Patchwork is already working on this task's worktree".into());
        }
        Ok(Step { core: self, held })
    }

    /// Whether a merge, update, discard, switch or cleanup is working on the worktree `chat`
    /// runs in: its own, or its parent's for a subagent. A turn doesn't start then.
    pub(crate) fn worktree_step_running(&self, chat: &Chat) -> bool {
        let owner = chat
            .parent
            .as_ref()
            .map_or(chat.id.as_str(), |p| p.chat_id.as_str());
        let held = self.hold_key(owner);
        self.worktree_steps.locked().contains(&held)
    }

    /// Holds the worktree chat `chat_id` works in, its own or its parent task's, for a step
    /// that rewrites its files, refused with `while_running` while the task or one of its
    /// subagents is running or starting. None when the chat works in no worktree.
    pub(crate) fn hold_turn_folder(
        &self,
        chat_id: &str,
        while_running: &str,
    ) -> Result<Option<Step<'_>>, Error> {
        let chat = self.store.chat(chat_id).ok_or(NOT_FOUND)?;
        let owner = chat.parent.as_ref().map_or(chat_id, |p| p.chat_id.as_str());
        if self.chat_worktree(owner).is_none() {
            return Ok(None);
        }
        let step = self.hold_worktree(owner)?;
        let sharing = self.sharing(owner);
        let running = self.store.chats().iter().any(|t| {
            let ours = sharing.contains(&t.id)
                || t.parent
                    .as_ref()
                    .is_some_and(|p| sharing.contains(&p.chat_id));
            ours && (self.runner.is_running(&t.id) || self.runner.is_starting(&t.id))
        });
        if running {
            return Err(while_running.to_owned().into());
        }
        Ok(Some(step))
    }

    /// Holds the folder Source Control commits or pulls in, `root`, for one step. In the open
    /// task `chat_id`'s worktree that is the hold of [`Self::hold_turn_folder`]; in a project's
    /// own folder on this computer, the lock merges into it take, so a task's merge doesn't land
    /// in the middle.
    pub(crate) fn hold_source_folder(
        &self,
        root: &str,
        chat_id: Option<&str>,
        while_running: &str,
    ) -> Result<SourceHold<'_>, Error> {
        let in_worktree = chat_id.is_some_and(|id| {
            let chat = self.store.chat(id);
            let owner = chat
                .as_ref()
                .and_then(|t| t.parent.as_ref())
                .map_or(id, |p| p.chat_id.as_str());
            self.chat_worktree(owner).is_some_and(|w| w.root == root)
        });
        if let Some(id) = chat_id.filter(|_| in_worktree) {
            return Ok(SourceHold {
                _step: self.hold_turn_folder(id, while_running)?,
                _merging: None,
            });
        }
        let merging = if ssh::is_remote(root) {
            None
        } else {
            let top = git(root, &["rev-parse", "--show-toplevel"])?;
            Some(merge_lock(top.trim(), || {})?)
        };
        Ok(SourceHold {
            _step: None,
            _merging: merging,
        })
    }

    /// Commits whatever the worktree has that isn't committed, as `message`. Returns the new
    /// commit, or None when there was nothing to commit.
    pub(super) fn commit_worktree(
        worktree: &Worktree,
        message: &str,
    ) -> Result<Option<String>, Error> {
        in_worktree(worktree, &["add", "-A"])?;
        if in_worktree(worktree, &["diff", "--cached", "--quiet"]).is_ok() {
            return Ok(None);
        }
        // A repository without a name to commit under still takes Patchwork's.
        let mut args = identity(&worktree.top);
        args.extend(["commit", "--no-verify", "-q", "-m", message]);
        in_worktree(worktree, &args)?;
        Ok(Some(
            in_worktree(worktree, &["rev-parse", "HEAD"])?
                .trim()
                .to_owned(),
        ))
    }

    /// Records `hash`, committed in the folder `root` by a merge or update, as task
    /// `chat_id`'s commit for the graph. A failure is logged: the commit stands either way.
    fn record_step_commit(&self, root: &str, chat_id: &str, hash: Option<&str>) {
        if let Some(hash) = hash
            && let Err(e) = self.record_commit(root, chat_id, hash)
        {
            log::warn!(hash:%, error:% = e; "could not record the task of commit {{hash}}: {{error}}");
        }
    }

    /// Merges task `chat_id`'s work into the branch its project is on, as a merge commit: what
    /// it hasn't committed is committed first, under the task's title. A merge that would
    /// conflict is undone and leaves the project as it was. Returns the branch merged into.
    /// `waiting` is called when another merge into the same repository, from this app or another
    /// Patchwork, holds it up, so the caller can say what the merge waits for.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has no worktree or is still running, when the project has
    /// changes the merge would overwrite, when the merge conflicts, or when git fails.
    pub fn merge_chat(&self, chat_id: &str, waiting: impl FnOnce()) -> Result<String, Error> {
        let worktree = self
            .chat_worktree(chat_id)
            .ok_or("This task doesn't have a worktree")?;
        let _step = self.begin_step(
            chat_id,
            "Wait for the task and its background commands to finish before merging it",
        )?;
        let title = self.chat(chat_id)?.title;
        let top = self.verify(chat_id, &worktree)?;
        let _merging = merge_lock(&top, waiting)?;
        // Git would refuse to merge anyway, and undoing that refusal would undo the user's own.
        if in_progress(&top) {
            return Err(
                "Finish or abort the merge, rebase or cherry-pick in progress in the project first"
                    .into(),
            );
        }
        // Committing in the middle of the worktree's own merge would record its conflicts.
        if in_progress(&worktree.top) {
            return Err(
                "Finish or abort the merge, rebase or cherry-pick in progress in the task's worktree first"
                    .into(),
            );
        }
        let into = source_control::git_branch(&top)
            .ok_or("The project is on a detached HEAD. Check out the branch to merge into first")?;
        if let Some(base) = worktree.base.as_ref().filter(|base| **base != into) {
            return Err(format!(
                "The project is on {into}, but this task started from {base}. Check out {base} to merge into it"
            )
            .into());
        }
        let committed = Self::commit_worktree(&worktree, &title)?;
        self.record_step_commit(&worktree.root, chat_id, committed.as_deref());
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
            let merged = git(&top, &["rev-parse", "HEAD"]).ok();
            self.record_step_commit(&top, chat_id, merged.as_deref().map(str::trim));
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

    /// Brings the commits on the project's branch that task `chat_id`'s worktree doesn't have
    /// into its branch, as a merge commit, so merging it later meets no surprises: what it hasn't
    /// committed is committed first, under the task's title. A merge that would conflict is
    /// undone and leaves the worktree as it was. Returns the branch brought in.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has no worktree or is still running, when the merge
    /// conflicts, or when git fails.
    pub fn update_worktree(&self, chat_id: &str) -> Result<String, Error> {
        let worktree = self
            .chat_worktree(chat_id)
            .ok_or("This task doesn't have a worktree")?;
        let _step = self.begin_step(
            chat_id,
            "Wait for the task and its background commands to finish before updating it",
        )?;
        let title = self.chat(chat_id)?.title;
        let top = self.verify(chat_id, &worktree)?;
        if in_progress(&worktree.top) {
            return Err(
                "Finish or abort the merge, rebase or cherry-pick in progress in the task's worktree first"
                    .into(),
            );
        }
        let from = source_control::git_branch(&top).unwrap_or_else(|| "HEAD".into());
        let head = git(&top, &["rev-parse", "HEAD"])?;
        let head = head.trim();
        let committed = Self::commit_worktree(&worktree, &title)?;
        self.record_step_commit(&worktree.root, chat_id, committed.as_deref());
        let before = in_worktree(&worktree, &["rev-parse", "HEAD"])?;
        let message = format!("Merge {from} into {}", worktree.branch);
        let mut args = identity(&worktree.top);
        args.extend(["merge", "--no-edit", "--no-verify", "-m", &message, head]);
        let Err(e) = in_worktree(&worktree, &args) else {
            // Only a merge commit on top of the branch is the update's own; already up to date
            // or fast-forwarded, it made none.
            let parent = in_worktree(&worktree, &["rev-parse", "-q", "--verify", "HEAD^1"]);
            if parent.is_ok_and(|p| p.trim() == before.trim()) {
                let updated = in_worktree(&worktree, &["rev-parse", "HEAD"]).ok();
                self.record_step_commit(&worktree.root, chat_id, updated.as_deref().map(str::trim));
            }
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

    /// Throws away everything task `chat_id` changed in its worktree that its project doesn't
    /// have: uncommitted files, and commits not merged. Ignored files, such as builds, stay.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has no worktree or is still running, or when git fails.
    pub fn discard_chat_changes(&self, chat_id: &str) -> Result<(), Error> {
        let worktree = self
            .chat_worktree(chat_id)
            .ok_or("This task doesn't have a worktree")?;
        let _step = self.begin_step(
            chat_id,
            "Stop the task and its background commands before discarding its changes",
        )?;
        let top = self.verify(chat_id, &worktree)?;
        let head = git(&top, &["rev-parse", "HEAD"])?;
        let base = in_worktree(&worktree, &["merge-base", "HEAD", head.trim()])?;
        in_worktree(&worktree, &["reset", "-q", "--hard", base.trim()])?;
        in_worktree(&worktree, &["clean", "-q", "-fd"])?;
        Ok(())
    }
}
