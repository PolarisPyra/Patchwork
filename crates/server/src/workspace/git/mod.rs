//! Git reads and writes for a project folder. All functions are blocking.

// Rust guideline compliant 2026-10-09

mod changes;
mod commits;
mod remote;
mod run;

#[doc(inline)]
pub use changes::{accept_file, git_changes, git_file_diff, list_files, revert_file};
pub(crate) use commits::{stage_all, staged_summary, truncated};
#[doc(inline)]
pub use commits::{commit, git_log};
pub(crate) use remote::{CheckState, base_ref, branch_summary, check_state, create_pull_request, gh};
#[doc(inline)]
pub use remote::{git_sync_state, push};
pub(crate) use run::git;
#[doc(inline)]
pub use run::git_branch;
pub(super) use run::{has_head, is_untracked};
