//! Source Control: the project's git repository and the agents that work in it.
//!
//! `git` reads and changes repository state, `hunks` applies one change of a diff at a time, and
//! `conflicts` reads and settles merge conflicts. Those helpers are blocking. On top of them,
//! committing, pushing and opening pull requests; which chat and replies each commit came from;
//! and the prompts that hand a pull request's feedback, or a merge's conflicts, to an agent. A
//! message the user leaves empty, and a pull request's text, are written by the model that
//! writes titles.

// Rust guideline compliant 2026-10-09

mod actions;
mod commits;
mod conflicts;
mod git;
mod hunks;
mod prompts;
mod types;
mod write;

#[doc(inline)]
pub use types::{
    BlameRange, CommitDetail, CommitOptions, CommitReply, Conflict, DiffSide, FileChange,
    FileRevision, GitBranch, GitCommit, GitRef, GitStash, GitSyncState, HunkAction, Operation,
    PullRequest, RepoAction, Take,
};

#[doc(inline)]
pub use conflicts::{conflicts_in, resolve_conflict};
#[doc(inline)]
pub use git::{
    ASKPASS_ENV, CredentialKind, CredentialRequest, abort_operation, accept_file, add_remote,
    changes_since, checkout, cherry_pick, commit, commit_detail, commit_file_diff,
    continue_operation, create_branch, create_tag, delete_branch, delete_tag, diff_since,
    discard_unstaged, fetch, fetch_remote, file_log, git_blame, git_branch, git_branches,
    git_changes, git_file_diff, git_log, git_stashes, git_sync_state, has_conflict_markers,
    list_files, merge, pull, push, push_tags, rebase, remove_remote, rename_branch, reset_to,
    revert_commit, revert_file, run_askpass, set_credential_prompter, set_upstream, stash,
    stash_apply, stash_drop, sync, undo_commit, unstage_all, unstage_file,
};
pub(crate) use git::{
    CheckState, answer_askpass, base_ref, branch_summary, check_state, committed_since,
    create_pull_request, gh, git as run_git, ignore, landed, own_commands, pr_merged,
    pull_request_checks, pull_request_view, stage_for_commit, staged_summary, truncated,
    unattended_ssh,
};
#[doc(inline)]
pub use hunks::{apply_hunk, staged_hunks};
