//! Issues to start tasks from: the open issues of a project's GitHub repository, read with
//! `gh`, and the Linear issues assigned to the person, read with their API key.
//!
//! | File | Holds |
//! |---|---|
//! | `types.rs` | An issue, where it came from, and the first message of a task started on it |
//! | `github.rs` | The repository's open issues, from `gh issue list` |
//! | `linear.rs` | The Linear API key, and the open issues assigned to its owner |

// Rust guideline compliant 2026-10-09

mod github;
mod linear;
mod types;

#[doc(inline)]
pub use types::{Issue, IssueSource};
