//! The Issues view: the open project's GitHub issues and the Linear issues assigned to the
//! person, each with a button that starts a task on it.
//!
//! | File | Holds |
//! |---|---|
//! | `issue_lists.rs` | The lists as loaded, their loading off the UI thread, and the Linear key |
//! | `issue_view.rs` | The view drawn: a section per tracker and a row per issue |

// Rust guideline compliant 2026-10-09

mod issue_lists;
mod issue_view;

pub use issue_lists::IssueLists;
pub use issue_view::issues_view;
