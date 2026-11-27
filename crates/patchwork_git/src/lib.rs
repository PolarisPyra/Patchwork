//! Git repository operations, hunk staging, conflict resolution, and askpass helpers.
//!
//! All functions in this crate are blocking and operate on local or SSH-remote project paths
//! through `patchwork-platform`.

// Rust guideline compliant 2026-10-09

mod conflicts;
mod git;
mod hunks;
mod model;

#[doc(inline)]
pub use conflicts::{conflicts_in, resolve_conflict};
#[doc(inline)]
pub use git::{
    ASKPASS_ENV, CheckState, CredentialKind, CredentialRequest, abort_operation, accept_file,
    add_remote, answer_askpass, base_ref, branch_summary, changes_since, check_state, checkout,
    cherry_pick, commit, commit_detail, commit_file_diff, committed_since, continue_operation,
    create_branch, create_pull_request, create_tag, delete_branch, delete_tag, diff_since,
    discard_unstaged, fetch, fetch_remote, file_log, gh, git as run_git, git_blame, git_branch,
    git_branches, git_changes, git_file_diff, git_log, git_stashes, git_sync_state,
    has_conflict_markers, ignore, landed, list_files, merge, own_commands, pr_merged, pull,
    pull_request_checks, pull_request_view, push, push_tags, rebase, remove_remote, rename_branch,
    reset_to, revert_commit, revert_file, run_askpass, set_credential_prompter,
    set_relay_endpoint_provider, set_upstream, stage_for_commit, staged_summary, stash,
    stash_apply, stash_drop, sync, truncated, unattended_ssh, undo_commit, unstage_all,
    unstage_file,
};
#[doc(inline)]
pub use hunks::{apply_hunk, staged_hunks};
#[doc(inline)]
pub use model::{
    BlameRange, CommitDetail, CommitOptions, CommitReply, Conflict, DiffSide, FileChange,
    FileRevision, GitBranch, GitCommit, GitRef, GitStash, GitSyncState, HunkAction, Operation,
    PullRequest, RepoAction, Take,
};
