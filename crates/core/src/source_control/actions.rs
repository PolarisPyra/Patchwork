//! The repository actions of Source Control: branches, stashes, tags, remotes and the rest.

// Rust guideline compliant 2026-10-09

use crate::source_control::RepoAction;
use crate::{Core, Error};

impl Core {
    /// Does `action` to the repository Source Control shows, and returns what to say it did.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when the open task is running, when the
    /// action would move a task's worktree off its own branch, or when git refuses it.
    pub fn repo_action(
        &self,
        project_id: &str,
        chat_id: Option<&str>,
        action: RepoAction,
    ) -> Result<String, Error> {
        let root = self.source_root(project_id, chat_id)?;
        let _hold = self.hold_source_folder(
            &root,
            chat_id,
            "Wait for the task to finish before changing its worktree",
        )?;
        let switches = matches!(
            action,
            RepoAction::Checkout { .. }
                | RepoAction::CreateBranch { .. }
                | RepoAction::RenameBranch { .. }
        );
        if switches && self.project_root(project_id)? != root {
            return Err("A task's worktree stays on its own branch".into());
        }
        run_action(&root, action)
    }
}

/// Does `action` in the repository at `root`, and returns what to say it did.
fn run_action(root: &str, action: RepoAction) -> Result<String, Error> {
    use crate::source_control as ws;
    Ok(match action {
        RepoAction::Fetch { all } => format!("Fetched {}", ws::fetch(root, all)?),
        RepoAction::Sync => format!("Synced with {}", ws::sync(root)?),
        RepoAction::Push { remote, force } => {
            let to = ws::push(root, remote.as_deref(), force)?;
            format!("{} to {to}", if force { "Force pushed" } else { "Pushed" })
        }
        RepoAction::PushTags => format!("Pushed tags to {}", ws::push_tags(root)?),
        RepoAction::Checkout { name, detached } => {
            ws::checkout(root, &name, detached)?;
            format!("Checked out {name}")
        }
        RepoAction::CreateBranch { name, from } => {
            ws::create_branch(root, &name, from.as_deref())?;
            format!("Created {name}")
        }
        RepoAction::DeleteBranch { name, force } => {
            ws::delete_branch(root, &name, force)?;
            format!("Deleted {name}")
        }
        RepoAction::RenameBranch { name } => {
            ws::rename_branch(root, &name)?;
            format!("Renamed the branch to {name}")
        }
        RepoAction::SetUpstream { upstream } => {
            ws::set_upstream(root, &upstream)?;
            format!("Tracking {upstream}")
        }
        RepoAction::Merge { name } => {
            ws::merge(root, &name)?;
            format!("Merged {name}")
        }
        RepoAction::Rebase { onto } => {
            ws::rebase(root, &onto)?;
            format!("Rebased onto {onto}")
        }
        RepoAction::Continue => format!("Finished the {}", ws::continue_operation(root)?.name()),
        RepoAction::Abort => format!("Aborted the {}", ws::abort_operation(root)?.name()),
        RepoAction::CherryPick { hash } => {
            ws::cherry_pick(root, &hash)?;
            format!("Cherry-picked {}", short(&hash))
        }
        RepoAction::Revert { hash } => {
            ws::revert_commit(root, &hash)?;
            format!("Reverted {}", short(&hash))
        }
        RepoAction::UndoCommit => {
            let message = ws::undo_commit(root)?;
            format!("Undid {}", message.lines().next().unwrap_or_default())
        }
        RepoAction::Reset { hash, hard } => {
            ws::reset_to(root, &hash, hard)?;
            format!("Reset to {}", short(&hash))
        }
        RepoAction::Stash {
            message,
            untracked,
            staged_only,
        } => {
            ws::stash(root, &message, untracked, staged_only)?;
            "Stashed the changes".into()
        }
        RepoAction::StashApply { index, drop } => {
            ws::stash_apply(root, index, drop)?;
            if drop {
                "Popped the stash"
            } else {
                "Applied the stash"
            }
            .into()
        }
        RepoAction::StashDrop { index } => {
            ws::stash_drop(root, index)?;
            "Dropped the stash".into()
        }
        RepoAction::CreateTag { name, message, at } => {
            ws::create_tag(root, &name, &message, at.as_deref())?;
            format!("Tagged {name}")
        }
        RepoAction::DeleteTag { name } => {
            ws::delete_tag(root, &name)?;
            format!("Deleted the tag {name}")
        }
        RepoAction::AddRemote { name, url } => {
            ws::add_remote(root, &name, &url)?;
            format!("Added the remote {name}")
        }
        RepoAction::RemoveRemote { name } => {
            ws::remove_remote(root, &name)?;
            format!("Removed the remote {name}")
        }
        RepoAction::UnstageAll => {
            ws::unstage_all(root)?;
            "Unstaged everything".into()
        }
        RepoAction::Ignore { path } => {
            ws::ignore(root, &path)?;
            format!("Ignored {path}")
        }
    })
}

/// The first characters of a commit's hash, as a commit is named in a sentence.
fn short(hash: &str) -> String {
    hash.chars().take(7).collect()
}
