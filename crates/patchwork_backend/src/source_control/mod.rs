//! Source Control: the project's git repository and the agents that work in it.
//!
//! Low-level git operations, hunk staging, and conflict resolution live in
//! `patchwork-git`. This module adds `Core`-aware orchestration on top: committing,
//! pushing, opening pull requests, chat/reply commit attribution, and agent review/conflict
//! prompts.

// Rust guideline compliant 2026-10-09

mod actions;
mod commits;
mod prompts;
mod write;

#[doc(inline)]
pub use patchwork_git::{
    ASKPASS_ENV, BlameRange, CommitDetail, CommitOptions, CommitReply, Conflict, CredentialKind,
    CredentialRequest, DiffSide, FileChange, FileRevision, GitBranch, GitCommit, GitRef, GitStash,
    GitSyncState, HunkAction, Operation, PullRequest, RepoAction, Take, abort_operation,
    accept_file, add_remote, changes_since, checkout, cherry_pick, commit, commit_detail,
    commit_file_diff, conflicts_in, continue_operation, create_branch, create_tag, delete_branch,
    delete_tag, diff_since, discard_unstaged, fetch, fetch_remote, file_log, git_blame, git_branch,
    git_branches, git_changes, git_file_diff, git_log, git_stashes, git_sync_state,
    has_conflict_markers, list_files, merge, pull, push, push_tags, rebase, remove_remote,
    rename_branch, reset_to, resolve_conflict, revert_commit, revert_file, run_askpass,
    set_upstream, stash, stash_apply, stash_drop, sync, undo_commit, unstage_all, unstage_file,
};
pub(crate) use patchwork_git::{
    CheckState, answer_askpass, base_ref, branch_summary, check_state, committed_since,
    create_pull_request, gh, ignore, landed, own_commands, pr_merged, pull_request_checks,
    pull_request_view, run_git, set_credential_prompter, set_relay_endpoint_provider,
    stage_for_commit, staged_summary, truncated, unattended_ssh,
};

use crate::Error;
use crate::chats::TurnTrees;

/// Stages, unstages, or discards the change of the file's diff whose text is `hunk`.
///
/// # Errors
///
/// Returns an error when the file's diff cannot be read, the change is no longer in it, the
/// change is not staged and cannot be unstaged, or `git apply` refuses it.
pub fn apply_hunk(
    root: &str,
    rel: &str,
    turn: Option<&TurnTrees>,
    hunk: &str,
    action: HunkAction,
) -> Result<(), Error> {
    patchwork_git::apply_hunk(
        root,
        rel,
        turn.map(|t| (t.before.as_str(), t.after.as_str())),
        hunk,
        action,
    )
}

/// Which changes of the file's diff are staged, in its order.
///
/// # Errors
///
/// Returns an error when the repository's top-level folder cannot be found or the file's diff
/// cannot be read.
pub fn staged_hunks(root: &str, rel: &str, turn: Option<&TurnTrees>) -> Result<Vec<bool>, Error> {
    patchwork_git::staged_hunks(
        root,
        rel,
        turn.map(|t| (t.before.as_str(), t.after.as_str())),
    )
}
