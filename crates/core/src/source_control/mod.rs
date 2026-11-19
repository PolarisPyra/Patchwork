//! Source Control git operations: commits, pushes, pull requests, and agent prompts.
//!
//! Git from the Source Control view, with the agents in it: committing, pushing, and opening
//! pull requests; which thread and replies each commit came from; and the prompts that hand a
//! pull request's feedback, or a merge's conflicts, to an agent. A message the user leaves
//! empty, and a pull request's text, are written by the model that writes titles.

// Rust guideline compliant 2026-10-09

mod actions;
mod commits;
mod prompts;
mod write;
