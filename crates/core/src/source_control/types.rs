//! Git changes in the open project.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    /// "modified" | "added" | "deleted" | "untracked" | "renamed" | "conflicted"
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub staged: bool,
    /// For a staged file the working tree changed again since: what that change is, named as
    /// `status` is ("modified" | "deleted").
    #[serde(default)]
    pub worktree: Option<String>,
    /// The path a renamed file had before, named from the project.
    #[serde(default)]
    pub renamed_from: Option<String>,
    /// How a conflicted file conflicts, such as "both modified" or "deleted by them".
    #[serde(default)]
    pub conflict: Option<String>,
    /// The lines of the staged part and of the part not staged, each as (added, deleted), for a
    /// file that has both. `additions` and `deletions` count the two together.
    #[serde(default)]
    pub parts: Option<((u32, u32), (u32, u32))>,
}

/// Where the project's branch stands against its remote, for the commit and push button.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate fact about the branch that the push button reads"
)]
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
    /// The default branch already has everything the branch changed, as it has once its pull
    /// request is merged, so there is nothing to push or open a pull request for.
    pub merged: bool,
    /// The open pull request for the branch, when `gh` is installed and signed in.
    pub pull_request: Option<PullRequest>,
    /// What git stopped in the middle of, to continue or abort.
    pub operation: Option<Operation>,
    /// The repository's remotes that can be pushed to, `origin` first.
    pub remotes: Vec<String>,
    /// The message git prepared for the next commit, as a merge or a squash leaves one.
    pub prepared_message: Option<String>,
}

/// What git stopped in the middle of, waiting for conflicts to be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    Rebase,
    Merge,
    CherryPick,
    Revert,
}

impl Operation {
    /// Its name in a sentence, such as "rebase".
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Rebase => "rebase",
            Self::Merge => "merge",
            Self::CherryPick => "cherry-pick",
            Self::Revert => "revert",
        }
    }
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

/// A run of a file's lines that one commit last changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameRange {
    /// The run's first line, counted from zero.
    pub start: usize,
    pub lines: usize,
    /// The commit, or nothing for lines not committed yet.
    pub hash: String,
    pub author: String,
    /// Author time, in seconds since the epoch.
    pub time: i64,
    pub subject: String,
}

/// A commit that changed one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRevision {
    pub hash: String,
    pub author: String,
    /// Author time, in seconds since the epoch.
    pub time: i64,
    pub subject: String,
    /// The file's name in that commit, which a later rename changed.
    pub path: String,
}

/// One conflict in a file, by the lines of its markers, counted from zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    /// The line that opens it. The branch's own lines follow.
    pub opens: usize,
    /// The line that opens the common ancestor's lines, when the file has them.
    pub ancestor: Option<usize>,
    /// The line between the two sides. The incoming lines follow.
    pub divides: usize,
    /// The line that closes it.
    pub closes: usize,
    /// What the branch's own side is called, such as `HEAD`.
    pub current: String,
    /// What the incoming side is called, such as the branch being merged.
    pub incoming: String,
}

/// Which lines of a conflict are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Take {
    /// The branch's own lines.
    Current,
    /// The lines coming in.
    Incoming,
    /// The branch's own lines, then the ones coming in.
    Both,
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
    #[serde(alias = "threadId")]
    pub chat_id: Option<String>,
    /// The replies of that chat whose changes it took in, oldest first.
    pub replies: Vec<CommitReply>,
}

/// What a commit holds beyond its row in the graph.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitDetail {
    /// The message under the subject.
    pub body: String,
    pub email: String,
    /// The files it changed against its first parent, named from the repository's top.
    pub files: Vec<FileChange>,
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

/// A branch or tag of the repository.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranch {
    /// Short name, such as `main`, `origin/main`, or `v1.0`.
    pub name: String,
    /// "local" | "remote" | "tag"
    pub kind: String,
    /// Whether it is the branch checked out.
    pub current: bool,
    /// The branch it tracks, such as `origin/main`.
    pub upstream: Option<String>,
    /// The subject of the commit it points at.
    pub subject: String,
}

/// An entry of the stash.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStash {
    /// Its place in the stash, the newest being 0.
    pub index: usize,
    pub message: String,
}

/// How a commit is made, beyond its message.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is one of git commit's own switches"
)]
pub struct CommitOptions {
    /// Replaces the last commit rather than adding one.
    pub amend: bool,
    /// Adds a `Signed-off-by` line.
    pub signoff: bool,
    /// Skips the repository's commit hooks.
    pub no_verify: bool,
    /// Stages every change first, even when something is staged already.
    pub all: bool,
    /// Commits even when nothing changed.
    pub allow_empty: bool,
}

/// Which part of a file's change a diff shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffSide {
    /// Everything since the last commit.
    #[default]
    All,
    /// What is staged, against the last commit.
    Staged,
    /// What is not staged, against what is.
    Unstaged,
}

/// One thing Source Control does to the repository as a whole.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RepoAction {
    /// Fetches the branch's remote, or every remote, pruning branches deleted there.
    Fetch {
        all: bool,
    },
    /// Pulls, then pushes.
    Sync,
    /// Pushes to `remote`, or to the upstream. `force` replaces the remote's branch unless it
    /// moved since it was last fetched.
    Push {
        remote: Option<String>,
        force: bool,
    },
    /// Pushes every tag.
    PushTags,
    /// Checks out a branch, a remote branch as a local one tracking it, or a tag or commit
    /// detached.
    Checkout {
        name: String,
        detached: bool,
    },
    /// Makes a branch at `from`, or at HEAD, and checks it out.
    CreateBranch {
        name: String,
        from: Option<String>,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    /// Renames the checked-out branch.
    RenameBranch {
        name: String,
    },
    /// Sets which remote branch the checked-out one tracks.
    SetUpstream {
        upstream: String,
    },
    Merge {
        name: String,
    },
    Rebase {
        onto: String,
    },
    /// Goes on with the stopped rebase, merge, cherry-pick or revert.
    Continue,
    Abort,
    CherryPick {
        hash: String,
    },
    Revert {
        hash: String,
    },
    /// Takes the last commit back, leaving its changes staged.
    UndoCommit,
    /// Moves the branch to `hash`. `hard` also throws away every change since.
    Reset {
        hash: String,
        hard: bool,
    },
    Stash {
        message: String,
        untracked: bool,
        staged_only: bool,
    },
    StashApply {
        index: usize,
        drop: bool,
    },
    StashDrop {
        index: usize,
    },
    CreateTag {
        name: String,
        message: String,
        at: Option<String>,
    },
    DeleteTag {
        name: String,
    },
    AddRemote {
        name: String,
        url: String,
    },
    RemoveRemote {
        name: String,
    },
    UnstageAll,
    /// Adds a path to the project's `.gitignore`.
    Ignore {
        path: String,
    },
}

/// What to do with one change of a file's diff.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HunkAction {
    Stage,
    Unstage,
    Discard,
}
