//! Filesystem, git, and process helpers. All functions are blocking.
//!
//! Split by concern: `fs` for project folders, `git` for repository state, `hunks` for one change of a diff at a time, `media` for how a
//! reply's files are shown, and `process` for signals and the editor. Callers use these re-exports.

mod types;

pub use types::{
    CommitReply, FileChange, GitCommit, GitRef, GitSyncState, HunkAction, ProjectChanged,
    PullRequest,
};

mod fs;
mod git;
mod hunks;
mod media;
mod process;
#[cfg(test)]
mod tests;

pub use fs::{create_project_dir, make_project, missing_files, read_file, resolve_inside};
pub use hunks::{apply_hunk, staged_hunks};
pub use git::{
    accept_file, branch_summary, commit, create_pull_request, git_branch, git_changes,
    git_file_diff, git_log, git_sync_state, list_files, push, revert_file, stage_all,
    staged_summary,
};
pub(crate) use git::{CheckState, base_ref, check_state, gh, git as run_git, truncated};
pub use media::{FileMedia, PreviewKind, file_media};
pub use process::{SIGKILL, SIGTERM, open_in_editor, signal_process_group};
