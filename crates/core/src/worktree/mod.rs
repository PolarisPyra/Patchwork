//! Worktrees: a task's own git worktree.
//!
//! A task in a git project gets one on its first turn, so tasks that run side by side in a project
//! never edit the same files. Its work reaches the project when the user merges it, and leaves
//! nothing behind when they discard it.
//!
//! Patchwork's own git commands here run with the repository's hooks and file monitor turned
//! off: an agent can write those, and they would otherwise run when the user clicks Merge.

// Rust guideline compliant 2026-10-09

mod cleanup;
mod condition;
mod create;
mod git;
mod home;
mod include;
mod landed;
mod remove;
mod settle;
mod share;
mod state;
mod steps;

pub(crate) use cleanup::ignored_only_here;
pub(crate) use home::{PROJECT_DIR, keep_out_of_git, make_home};
#[doc(inline)]
pub use state::{BranchCompare, Worktree, WorktreeMark, WorktreeState};
