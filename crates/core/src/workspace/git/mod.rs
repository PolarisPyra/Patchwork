//! Git reads and writes for a project folder. All functions are blocking.

// Rust guideline compliant 2026-10-09

mod askpass;
mod branches;
mod changes;
mod commits;
mod file_history;
mod operation;
mod remote;
mod run;
mod stash;
mod sync;

#[doc(inline)]
pub use askpass::{
    ASKPASS_ENV, CredentialKind, CredentialRequest, run_askpass, set_credential_prompter,
};
pub(crate) use askpass::answer_askpass;
#[doc(inline)]
pub use branches::{
    add_remote, checkout, create_branch, create_tag, delete_branch, delete_tag, git_branches,
    remove_remote, rename_branch, set_upstream,
};
pub(crate) use changes::ignore;
#[doc(inline)]
pub use changes::{
    accept_file, discard_unstaged, git_changes, git_file_diff, has_conflict_markers, list_files,
    revert_file, unstage_all, unstage_file,
};
pub(crate) use commits::{committed_since, stage_for_commit, staged_summary, truncated};
#[doc(inline)]
pub use commits::{commit, commit_detail, git_log};
#[doc(inline)]
pub use file_history::{commit_file_diff, file_log, git_blame};
#[doc(inline)]
pub use operation::{
    abort_operation, cherry_pick, continue_operation, merge, rebase, reset_to, revert_commit,
    undo_commit,
};
pub(crate) use remote::{
    CheckState, base_ref, branch_summary, check_state, create_pull_request, gh, landed, pr_merged,
    pull_request_checks, pull_request_view, unattended_ssh,
};
#[doc(inline)]
pub use remote::{fetch_remote, git_sync_state};
pub(crate) use run::{git, own_commands};
#[doc(inline)]
pub use run::git_branch;
pub(super) use run::{has_head, is_untracked};
#[doc(inline)]
pub use stash::{git_stashes, stash, stash_apply, stash_drop};
#[doc(inline)]
pub use sync::{fetch, pull, push, push_tags, sync};
