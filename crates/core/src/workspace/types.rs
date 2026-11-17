//! Git changes in the open project.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    /// "modified" | "added" | "deleted" | "untracked" | "renamed" | "conflicted"
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub staged: bool,
}

/// Where the project's branch stands against its remote, for the commit and push button.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSyncState {
    /// None outside a git repository, or on a detached HEAD.
    pub branch: Option<String>,
    /// The remote's default branch, such as `main`; None when it can't be told.
    pub default_branch: Option<String>,
    pub has_changes: bool,
    /// The repository has a remote to push to.
    pub has_remote: bool,
    pub has_upstream: bool,
    /// Commits on the branch that its upstream doesn't have.
    pub ahead: u32,
    pub behind: u32,
    /// The open pull request for the branch, when `gh` is installed and signed in.
    pub pull_request: Option<PullRequest>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub number: u32,
    pub url: String,
    /// How many of its checks failed, are still running, and passed.
    pub failed: u32,
    pub pending: u32,
    pub passed: u32,
    /// Comments and reviews left on it.
    pub comments: u32,
}

/// A commit in the Source Control graph.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    pub hash: String,
    /// Parent hashes, the first parent first; two or more for a merge.
    pub parents: Vec<String>,
    pub subject: String,
    pub author: String,
    /// Author time, in seconds since the epoch.
    pub time: i64,
    pub refs: Vec<GitRef>,
    /// Whether a remote branch has it. With no remote, every commit counts as pushed.
    pub pushed: bool,
    /// The chat or task it was committed from in Patchwork, if it was.
    pub thread_id: Option<String>,
    /// The replies of that thread whose changes it took in, oldest first.
    pub replies: Vec<CommitReply>,
}

/// A reply whose changes went into a commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitReply {
    pub message_id: String,
    /// The first line of the request it answered.
    pub prompt: String,
}

/// A name that points at a commit.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRef {
    /// Short name, such as `main`, `origin/main`, or `v1.0`.
    pub name: String,
    /// "head" (the checked-out branch) | "branch" | "remote" | "tag"
    pub kind: String,
}

/// A settled burst of file changes in the watched project, with git's state after it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectChanged {
    pub project_id: String,
    /// Project-relative paths the burst touched; `.git/...` for commits, staging and checkouts.
    pub paths: Vec<String>,
    pub branch: Option<String>,
    /// None when `git status` failed, so the last good list stays.
    pub changes: Option<Vec<FileChange>>,
}

/// What to do with one change of a file's diff.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HunkAction {
    Stage,
    Unstage,
    Discard,
}
