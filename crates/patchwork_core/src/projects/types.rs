//! A project, the folders that group its chats, and the watcher on the folder Source Control
//! shows.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

use crate::provider::AgentKind;
use crate::source_control::FileChange;
use crate::system::ssh;

use super::watch::ProjectWatcher;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
}

impl Project {
    /// The SSH host the project is on, by its `Host` alias; none for this computer.
    #[must_use]
    pub fn host(&self) -> Option<&str> {
        ssh::split(&self.path).map(|l| l.host)
    }
}

/// A folder inside a project. Its chats are those whose `folder_id` names it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub project_id: String,
    pub name: String,
    /// The agent whose tasks it holds, when sorting tasks by provider made it.
    #[serde(default)]
    pub sorted_by: Option<AgentKind>,
}

/// The watcher on the folder Source Control shows, and what it was started for.
#[derive(Debug)]
pub(crate) struct ProjectWatch {
    /// The `seq` of the newest `watch_project` call that asked for this folder.
    pub(super) seq: u64,
    pub(super) project_id: String,
    pub(super) folder: String,
    pub(super) _watcher: ProjectWatcher,
    /// While a task's worktree is shown, the watcher on the project's own folder that credits
    /// replies with what they change.
    pub(super) _credit: Option<ProjectWatcher>,
}

/// A settled burst of file changes in the watched project, with git's state after it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectChanged {
    pub project_id: String,
    /// The folder watched: the project's, or the open task's worktree.
    pub root: String,
    /// Project-relative paths the burst touched; `.git/...` for commits, staging and checkouts.
    pub paths: Vec<String>,
    pub branch: Option<String>,
    /// None when `git status` failed, so the last good list stays.
    pub changes: Option<Vec<FileChange>>,
}
