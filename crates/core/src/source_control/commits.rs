//! Committing and pulling from Source Control, and which task and replies each commit came from.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::threads::Role;
use crate::workspace::{self, CommitOptions, CommitReply, GitCommit};
use crate::{Core, Error};

/// The `app_state` key of what was recorded about a commit Patchwork made, before its hash.
const COMMIT_THREAD: &str = "commit-thread:";
/// The `app_state` key of a thread's newest reply that went into a commit, before its id.
const LAST_COMMITTED: &str = "thread-committed:";

/// What was recorded about a commit made from a thread.
#[derive(Serialize, Deserialize)]
pub(super) struct CommitRecord {
    pub(super) thread: String,
    pub(super) replies: Vec<CommitReply>,
}

impl Core {
    /// The folder Source Control shows for project `project_id`: the worktree of the open task
    /// `thread_id` when the task is the project's and works in one, else the project's folder.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown.
    pub fn source_root(&self, project_id: &str, thread_id: Option<&str>) -> Result<String, Error> {
        let task = thread_id
            .and_then(|id| self.store.thread(id))
            .filter(|t| t.project_id.as_deref() == Some(project_id));
        match task {
            Some(task) => self.thread_root(&task.id),
            None => self.project_root(project_id),
        }
    }

    /// Commits what is staged with `message`, or every change when nothing is staged or
    /// `options` asks for all of it. A blank `message` is written by a model from the staged
    /// patch, or kept from the last commit when amending. The commit is recorded as the open thread's, with
    /// its replies whose changes it took in, for the graph. Returns the commit's short hash and
    /// its subject.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when `git` fails to stage or commit, or when
    /// `message` is blank and no model writes one.
    pub fn commit_changes(
        &self,
        project_id: &str,
        thread_id: Option<&str>,
        message: &str,
        options: CommitOptions,
    ) -> Result<(String, String), Error> {
        let root = self.source_root(project_id, thread_id)?;
        let _hold = self.hold_source_folder(
            &root,
            thread_id,
            "Wait for the task to finish before committing its worktree",
        )?;
        if !options.allow_empty {
            workspace::stage_for_commit(&root, options.all)?;
        }
        let message = match message.trim() {
            "" if options.amend => String::new(),
            "" => self.staged_message(&root, thread_id)?,
            typed => typed.to_string(),
        };
        let hash = workspace::commit(&root, &message, options)?;
        if let Some(thread_id) = thread_id
            && let Err(e) = self.record_commit(&root, thread_id, &hash)
        {
            log::warn!(
                hash:%, error:% = e;
                "could not record the thread of commit {{hash}}: {{error}}"
            );
        }
        let subject = workspace::run_git(&root, &["log", "-1", "--format=%s"])?
            .trim()
            .to_string();
        Ok((hash.chars().take(7).collect(), subject))
    }

    /// Pulls the commits the upstream of the branch Source Control shows has. Returns the
    /// upstream's name.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when the open task is running, when the
    /// branch has no upstream, or when the pull fails or stops on conflicts.
    pub fn pull_changes(&self, project_id: &str, thread_id: Option<&str>) -> Result<String, Error> {
        let root = self.source_root(project_id, thread_id)?;
        let _hold = self.hold_source_folder(
            &root,
            thread_id,
            "Wait for the task to finish before pulling into its worktree",
        )?;
        workspace::pull(&root)
    }

    /// Records that `hash` was committed from the thread, with the replies since its last
    /// commit that changed a file the commit has. A merge commit's files are those it brought
    /// onto its first parent.
    pub(crate) fn record_commit(
        &self,
        root: &str,
        thread_id: &str,
        hash: &str,
    ) -> Result<(), Error> {
        let files = workspace::run_git(
            root,
            &[
                "show",
                "-z",
                "--name-only",
                "--relative",
                "--diff-merges=first-parent",
                "--format=",
                hash,
            ],
        )?;
        // Unquoted, so a name with spaces or accents matches the reply's own.
        let files: HashSet<&str> = files
            .split('\0')
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .collect();
        let last = self.store.state(&format!("{LAST_COMMITTED}{thread_id}"));
        let replies = self
            .store
            .thread(thread_id)
            .map(|thread| {
                let messages = &thread.messages;
                let after = last
                    .and_then(|id| messages.iter().position(|m| m.id == id))
                    .map_or(0, |i| i + 1);
                (after..messages.len())
                    .filter(|&i| {
                        let m = &messages[i];
                        m.role == Role::Assistant
                            && m.turn.is_some()
                            && m.files.iter().any(|f| files.contains(f.as_str()))
                    })
                    .map(|i| CommitReply {
                        message_id: messages[i].id.clone(),
                        prompt: messages[..i]
                            .iter()
                            .rev()
                            .find(|m| m.role == Role::User)
                            .and_then(|m| m.text.lines().find(|l| !l.trim().is_empty()))
                            .map(|l| l.trim().chars().take(80).collect())
                            .unwrap_or_default(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if let Some(newest) = replies.last() {
            self.store
                .set_state(&format!("{LAST_COMMITTED}{thread_id}"), &newest.message_id)?;
        }
        let record = CommitRecord {
            thread: thread_id.to_string(),
            replies,
        };
        let record = serde_json::to_string(&record).map_err(|e| e.to_string())?;
        self.store
            .set_state(&format!("{COMMIT_THREAD}{hash}"), &record)
    }

    /// What was recorded about a commit Patchwork made from a thread.
    pub(super) fn commit_record(&self, hash: &str) -> Option<CommitRecord> {
        let saved = self.store.state(&format!("{COMMIT_THREAD}{hash}"))?;
        // The first builds kept only the thread's id.
        Some(serde_json::from_str(&saved).unwrap_or(CommitRecord {
            thread: saved,
            replies: Vec::new(),
        }))
    }

    /// The branch's and its upstream's commits, each with the thread Patchwork committed it
    /// from and that thread's replies in it, if any.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown or `git log` fails.
    pub fn git_log(
        &self,
        project_id: &str,
        thread_id: Option<&str>,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<GitCommit>, Error> {
        let root = self.source_root(project_id, thread_id)?;
        let mut commits = workspace::git_log(&root, skip, limit)?;
        for commit in &mut commits {
            if let Some(record) = self.commit_record(&commit.hash) {
                commit.thread_id = Some(record.thread);
                commit.replies = record.replies;
            }
        }
        Ok(commits)
    }
}
