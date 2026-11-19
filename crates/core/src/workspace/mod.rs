//! Filesystem, git, and process helpers. All functions are blocking.
//!
//! Split by concern: `fs` for project folders, `git` for repository state, `hunks` for one change of a diff at a time, `media` for how a
//! reply's files are shown, `process` for the editor, and `watch` for changes to the open project. Callers use these re-exports.

// Rust guideline compliant 2026-10-09

mod types;

#[doc(inline)]
pub use types::{
    BlameRange, Conflict, FileRevision, Take, CommitDetail, CommitOptions, CommitReply, DiffSide,
    FileChange, GitBranch, GitCommit, GitRef, GitStash, GitSyncState, HunkAction, Operation,
    ProjectChanged, PullRequest, RepoAction,
};

mod conflicts;
mod fs;
mod git;
mod hunks;
mod media;
mod process;
pub mod watch;
#[cfg(test)]
mod tests;

#[doc(inline)]
pub use fs::{missing_files, read_file, resolve_inside};
pub(crate) use fs::{create_project_dir, make_project};
#[doc(inline)]
pub use conflicts::{conflicts_in, resolve_conflict};
#[doc(inline)]
pub use hunks::{apply_hunk, staged_hunks};
#[doc(inline)]
pub use git::{
    ASKPASS_ENV, CredentialKind, CredentialRequest, run_askpass, set_credential_prompter,
    abort_operation, accept_file, add_remote, checkout, cherry_pick, commit, commit_detail,
    commit_file_diff, file_log, git_blame, continue_operation, create_branch, create_tag,
    delete_branch, delete_tag, discard_unstaged, fetch, fetch_remote, git_branch, git_branches,
    git_changes, git_file_diff, git_log, git_stashes, git_sync_state, has_conflict_markers,
    list_files, merge, pull, push, push_tags, rebase, remove_remote, rename_branch, reset_to,
    revert_commit, revert_file, set_upstream, stash, stash_apply, stash_drop, sync, undo_commit,
    unstage_all, unstage_file,
};
pub(crate) use git::{
    CheckState, answer_askpass, base_ref, branch_summary, check_state, committed_since,
    create_pull_request, gh, git as run_git, ignore, landed, own_commands, pr_merged,
    pull_request_checks, pull_request_view, stage_for_commit, staged_summary, truncated,
    unattended_ssh,
};
#[doc(inline)]
pub use media::{FileMedia, PreviewKind, file_media};
#[doc(inline)]
pub use process::open_in_editor;
