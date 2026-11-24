//! Issues to start tasks from: the open issues of a project's GitHub repository, read with `gh`.
//!
//! | File | Holds |
//! |---|---|
//! | `types.rs` | An issue, and the first message of a task started on it |
//! | `github.rs` | The repository's open issues, from `gh issue list` |

// Rust guideline compliant 2026-10-09

mod github;
mod types;

#[doc(inline)]
pub use types::Issue;
