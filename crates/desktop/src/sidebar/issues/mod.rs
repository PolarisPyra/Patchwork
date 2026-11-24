//! The Issues view: the open project's GitHub issues, each with a button that starts a task on
//! it.
//!
//! | File | Holds |
//! |---|---|
//! | `issue_lists.rs` | The list as loaded, and its loading off the UI thread |
//! | `issue_view.rs` | The view drawn: the repository's section and a row per issue |

// Rust guideline compliant 2026-10-09

mod issue_lists;
mod issue_view;

pub use issue_lists::IssueLists;
pub use issue_view::issues_view;
