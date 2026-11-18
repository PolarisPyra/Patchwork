//! Filesystem, git, and process helpers. All functions are blocking.
//!
//! Split by concern: `fs` for project folders, `git` for repository state, `hunks` for one change of a diff at a time, `media` for how a
//! reply's files are shown, `process` for the editor, and `watch` for changes to the open project. Callers use these re-exports.

// Rust guideline compliant 2026-10-09

mod types;

#[doc(inline)]
pub use types::{
    CommitReply, FileChange, GitCommit, GitRef, GitSyncState, HunkAction, ProjectChanged,
    PullRequest,
};

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
pub use hunks::{apply_hunk, staged_hunks};
#[doc(inline)]
pub use git::{
    accept_file, commit, git_branch, git_changes, git_file_diff, git_log, git_sync_state,
    list_files, pull, push, revert_file,
};
pub(crate) use git::{
    CheckState, base_ref, branch_summary, check_state, create_pull_request, fetch_remote, gh,
    git as run_git, landed, pull_request_checks, pull_request_view, stage_all, staged_summary,
    truncated, unattended_ssh,
};
#[doc(inline)]
pub use media::{FileMedia, PreviewKind, file_media};
#[doc(inline)]
pub use process::open_in_editor;
