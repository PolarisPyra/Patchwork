//! Snapshots of a project's files, taken as each turn starts, and restoring one. A snapshot is a
//! git tree of every file git does not ignore, built in a temporary index so the user's own index
//! and branches are untouched, and kept alive by a ref under `refs/patchwork/checkpoints`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::process::{Command, Output};

use crate::Res;
use crate::Server;
use crate::types::new_id;
use crate::types::Thread;

const REFS: &str = "refs/patchwork/checkpoints";

fn thread_refs(thread_id: &str) -> String {
    format!("{REFS}/{thread_id}/")
}

fn ref_name(thread_id: &str, message_id: &str) -> String {
    format!("{}{message_id}", thread_refs(thread_id))
}

/// Runs git in `root`, reading the index from `index` when given, and writing `input` to its stdin.
fn run(root: &str, index: Option<&Path>, args: &[&str], input: Option<&[u8]>) -> Res<Output> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(root)
        .env("GIT_LITERAL_PATHSPECS", "1")
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index);
    }
    let mut child = cmd.spawn().map_err(|e| format!("Failed to run git: {e}"))?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(input).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(out)
    } else {
        Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn git(root: &str, args: &[&str]) -> Res<String> {
    let out = run(root, None, args, None)?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A tree of the project's files as they are now. The user's index is copied first, so git only
/// re-reads the files that changed since it was last refreshed.
fn snapshot(root: &str) -> Res<String> {
    let index = PathBuf::from(root).join(git(root, &["rev-parse", "--git-path", "index"])?);
    let temp = PathBuf::from(root).join(git(
        root,
        &["rev-parse", "--git-path", &format!("patchwork-index-{}", new_id())],
    )?);
    if index.exists() {
        std::fs::copy(&index, &temp).map_err(|e| e.to_string())?;
    }
    let tree = run(root, Some(&temp), &["add", "-A"], None)
        .and_then(|_| run(root, Some(&temp), &["write-tree"], None));
    let _ = std::fs::remove_file(&temp);
    Ok(String::from_utf8_lossy(&tree?.stdout).trim().to_string())
}

/// Snapshots the project before the turn for `message_id` changes it. Nothing is taken outside
/// a git repository.
pub(super) fn capture(root: &str, thread_id: &str, message_id: &str) {
    if git(root, &["rev-parse", "--git-dir"]).is_err() {
        return;
    }
    let taken = snapshot(root)
        .and_then(|tree| git(root, &["update-ref", &ref_name(thread_id, message_id), &tree]));
    if let Err(e) = taken {
        log::warn!("failed to take a checkpoint: {e}");
    }
}

/// The messages of the thread that have a snapshot from before their turn.
pub fn list(root: &str, thread_id: &str) -> Vec<String> {
    let prefix = thread_refs(thread_id);
    git(root, &["for-each-ref", "--format=%(refname)", &prefix])
        .unwrap_or_default()
        .lines()
        .filter_map(|name| name.strip_prefix(&prefix))
        .map(String::from)
        .collect()
}

/// Puts the project's files back as they were before the turn for `message_id`: files changed or
/// deleted since are rewritten, and files created since are deleted. Ignored files are left alone.
pub fn restore(root: &str, thread_id: &str, message_id: &str) -> Res<()> {
    let saved = git(root, &["rev-parse", "--verify", "-q", &ref_name(thread_id, message_id)])
        .map_err(|_| "This message has no checkpoint to restore")?;
    let now = snapshot(root)?;
    let diff = run(
        root,
        None,
        &["diff-tree", "-r", "-z", "--relative", "--no-renames", "--name-status", &saved, &now],
        None,
    )?;
    let fields: Vec<&[u8]> = diff.stdout.split(|&b| b == 0).collect();
    let mut rewrite = Vec::new();
    for pair in fields.chunks(2) {
        let [status, path] = pair else { continue };
        if status == b"A" {
            let path = Path::new(root).join(String::from_utf8_lossy(path).as_ref());
            std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        } else {
            rewrite.extend_from_slice(path);
            rewrite.push(0);
        }
    }
    if !rewrite.is_empty() {
        let source = format!("--source={saved}");
        let args = [
            "restore",
            &source,
            "--worktree",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
        ];
        run(root, None, &args, Some(&rewrite))?;
    }
    Ok(())
}

/// Drops the snapshots of the given messages, or of the whole thread when `message_ids` is None.
pub fn remove(root: &str, thread_id: &str, message_ids: Option<&[String]>) {
    let ids = match message_ids {
        Some(ids) => ids.to_vec(),
        None => list(root, thread_id),
    };
    ids.into_iter().for_each(|id| {
        let _ = git(root, &["update-ref", "-d", &ref_name(thread_id, &id)]);
    });
}

impl Server {
    /// The project root of a thread, for threads that belong to a project.
    fn checkpoint_root(&self, thread_id: &str) -> Option<String> {
        let project_id = self.store.thread(thread_id)?.project_id?;
        self.project_root(&project_id).ok()
    }

    /// Snapshots the project before the reply to `message_id`, for a message that an idle
    /// agent takes rather than a new turn.
    pub(crate) fn capture_checkpoint(&self, thread_id: &str, message_id: &str) {
        if let Some(root) = self.checkpoint_root(thread_id) {
            capture(&root, thread_id, message_id);
        }
    }

    /// The user messages of the thread whose turn can be undone with `restore_checkpoint`.
    pub fn checkpoints(&self, thread_id: &str) -> Vec<String> {
        self.checkpoint_root(thread_id)
            .map(|root| list(&root, thread_id))
            .unwrap_or_default()
    }

    /// Undoes the turn of the user message `message_id` and everything after it: the project's
    /// files go back to how they were before that turn, and the message and the ones after it
    /// leave the thread. The agent starts a new session next turn, given the remaining messages
    /// as a transcript, since its own session still holds the undone turns.
    pub fn restore_checkpoint(&self, thread_id: &str, message_id: &str) -> Res<Thread> {
        if self.runner.is_running(thread_id) {
            return Err("Stop the running turn before restoring a checkpoint".into());
        }
        // An idle agent still holds the session being undone.
        self.runner.retire_idle(thread_id);
        let root = self
            .checkpoint_root(thread_id)
            .ok_or("Only project tasks have checkpoints")?;
        let thread = self.thread(thread_id)?;
        let start = thread
            .messages
            .iter()
            .position(|m| m.id == message_id)
            .ok_or("Message not found")?;
        restore(&root, thread_id, message_id)?;
        let undone: Vec<String> = thread.messages[start..].iter().map(|m| m.id.clone()).collect();
        remove(&root, thread_id, Some(&undone));
        self.store.update_thread(thread_id, |t| {
            t.messages.truncate(start);
            t.session_id = None;
            t.goal = None;
            t.context_pending = !t.messages.is_empty();
        })?;
        self.thread(thread_id)
    }

    /// Drops the thread's checkpoints, once its messages are gone.
    pub(crate) fn remove_checkpoints(&self, thread_id: &str) {
        if let Some(root) = self.checkpoint_root(thread_id) {
            remove(&root, thread_id, None);
        }
    }
}
