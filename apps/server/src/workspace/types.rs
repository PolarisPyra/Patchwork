//! Git changes in the open project.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    /// "modified" | "added" | "deleted" | "untracked" | "renamed"
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub staged: bool,
}

/// A settled burst of file changes in the watched project, with git's state after it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ProjectChanged {
    pub project_id: String,
    /// Project-relative paths the burst touched; `.git/...` for commits, staging and checkouts.
    pub paths: Vec<String>,
    pub branch: Option<String>,
    /// None when `git status` failed, so the last good list stays.
    pub changes: Option<Vec<FileChange>>,
}
