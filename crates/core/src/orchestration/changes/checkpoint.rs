//! Snapshots of a project's files, taken as each turn starts and ends, and restoring one.
//!
//! A snapshot is a git tree of every file git does not ignore, built in a temporary index so the
//! user's own index and branches are untouched, and kept alive by a ref under `refs/patchwork`. The
//! pair around a turn is what the turn changed, whatever happens to the files later.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::process::Output;

use crate::Core;
use crate::system::new_id;
use crate::provider::AgentKind;
use crate::threads::{ResumePoint, Thread, TurnTrees};

const REFS: &str = "refs/patchwork/checkpoints";
/// Snapshots from the end of a turn, one ref per reply the turn wrote.
const END_REFS: &str = "refs/patchwork/turn-ends";
/// The project as it was just before a checkpoint was restored, one ref per thread, so a restore
/// can be taken back with git.
const PRE_RESTORE_REFS: &str = "refs/patchwork/pre-restore";

fn thread_refs(thread_id: &str) -> String {
    format!("{REFS}/{thread_id}/")
}

fn ref_name(thread_id: &str, message_id: &str) -> String {
    format!("{}{message_id}", thread_refs(thread_id))
}

fn end_ref(thread_id: &str, message_id: &str) -> String {
    format!("{END_REFS}/{thread_id}/{message_id}")
}

/// Runs git in `root`, reading the index from `index` when given, and writing `input` to its stdin.
pub(super) fn run(
    root: &str,
    index: Option<&Path>,
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<Output, Error> {
    let mut cmd = crate::system::host_command("git");
    cmd.args(args)
        .current_dir(root)
        .env("GIT_LITERAL_PATHSPECS", "1");
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index);
    }
    let mut child = crate::system::ssh::prepare(cmd)?
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run git: {e}"))?;
    // Written while the output is read: git answers a long input as it reads it, and with both
    // pipes full each side would wait on the other.
    let out = std::thread::scope(|scope| {
        if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
            scope.spawn(move || {
                // A git that stopped reading says why in its own error.
                let _ = stdin.write_all(input);
            });
        }
        child.wait_with_output()
    })
    .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(out)
    } else {
        Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )
        .into())
    }
}

pub(super) fn git(root: &str, args: &[&str]) -> Result<String, Error> {
    let out = run(root, None, args, None)?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A tree of the project's files as they are now. The user's index is copied first, so git only
/// re-reads the files that changed since it was last refreshed.
fn snapshot(root: &str) -> Result<String, Error> {
    if crate::system::ssh::is_remote(root) {
        return remote_snapshot(root);
    }
    let index = PathBuf::from(root).join(git(root, &["rev-parse", "--git-path", "index"])?);
    let temp = PathBuf::from(root).join(git(
        root,
        &[
            "rev-parse",
            "--git-path",
            &format!("patchwork-index-{}", new_id()),
        ],
    )?);
    if index.exists() {
        std::fs::copy(&index, &temp).map_err(|e| e.to_string())?;
    }
    let tree = run(root, Some(&temp), &["add", "-A"], None)
        .and_then(|_| run(root, Some(&temp), &["write-tree"], None));
    let _ = std::fs::remove_file(&temp);
    Ok(String::from_utf8_lossy(&tree?.stdout).trim().to_string())
}

/// [`snapshot`] for a project on an SSH host, where the temporary index is a file on the host.
fn remote_snapshot(root: &str) -> Result<String, Error> {
    let Some(at) = crate::system::ssh::split(root) else {
        return Err("Not a remote project".into());
    };
    crate::system::ssh::op::snapshot(at.host, at.path)
}

/// Snapshots the project before the turn for `message_id` changes it. Nothing is taken outside
/// a git repository.
pub(in crate::orchestration) fn capture(root: &str, thread_id: &str, message_id: &str) {
    if git(root, &["rev-parse", "--git-dir"]).is_err() {
        return;
    }
    let taken = snapshot(root).and_then(|tree| {
        git(
            root,
            &["update-ref", &ref_name(thread_id, message_id), &tree],
        )
    });
    if let Err(e) = taken {
        log::warn!(error:% = e; "failed to take a checkpoint: {{error}}");
    }
}

/// Snapshots the project as the turn after `user_message_id` ends, and pairs it with the snapshot
/// from before that turn. Each of `replies` keeps the end snapshot alive. None when the turn had
/// no snapshot before it.
pub(in crate::orchestration) fn record_end(
    root: &str,
    thread_id: &str,
    user_message_id: &str,
    replies: &[String],
) -> Option<TurnTrees> {
    let before = git(
        root,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &ref_name(thread_id, user_message_id),
        ],
    )
    .ok()?;
    let after = snapshot(root)
        .map_err(|e| log::warn!(error:% = e; "failed to snapshot a turn's end: {{error}}"))
        .ok()?;
    for id in replies {
        if let Err(e) = git(root, &["update-ref", &end_ref(thread_id, id), &after]) {
            log::warn!(error:% = e; "failed to record a turn's end: {{error}}");
        }
    }
    Some(TurnTrees { before, after })
}

/// What a turn still running has done so far to `paths`, commits it made included.
///
/// Pairs the snapshot from before the turn after `user_message_id` with that snapshot plus
/// `paths` as they are now. Only those files are read, so it costs what the turn touched, not
/// the whole project. None when the turn had no snapshot before it.
#[must_use]
pub fn turn_so_far(
    root: &str,
    thread_id: &str,
    user_message_id: &str,
    paths: &[String],
) -> Option<TurnTrees> {
    let before = git(
        root,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &ref_name(thread_id, user_message_id),
        ],
    )
    .ok()?;
    let after = touched_tree(root, &before, paths)
        .map_err(|e| log::warn!(error:% = e; "failed to read a running turn: {{error}}"))
        .ok()?;
    Some(TurnTrees { before, after })
}

/// `before` with `paths` as they are now on disk, leaving out ignored files as [`snapshot`] does.
fn touched_tree(root: &str, before: &str, paths: &[String]) -> Result<String, Error> {
    if crate::system::ssh::is_remote(root) {
        return snapshot(root);
    }
    let temp = PathBuf::from(root).join(git(
        root,
        &[
            "rev-parse",
            "--git-path",
            &format!("patchwork-index-{}", new_id()),
        ],
    )?);
    let tree = (|| {
        run(root, Some(&temp), &["read-tree", before], None)?;
        let names = paths.join("\n") + "\n";
        // check-ignore exits 1 when nothing is ignored.
        let ignored = run(
            root,
            Some(&temp),
            &["check-ignore", "--stdin"],
            Some(names.as_bytes()),
        )
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default();
        let ignored: std::collections::HashSet<&str> = ignored.lines().collect();
        let kept: Vec<&str> = paths
            .iter()
            .map(String::as_str)
            .filter(|p| !ignored.contains(p))
            .collect();
        let names = kept.join("\0") + "\0";
        run(
            root,
            Some(&temp),
            &["update-index", "--add", "--remove", "-z", "--stdin"],
            Some(names.as_bytes()),
        )?;
        git_with_index(root, &temp, &["write-tree"])
    })();
    let _ = std::fs::remove_file(&temp);
    tree
}

/// [`git`] against the index at `index`.
fn git_with_index(root: &str, index: &Path, args: &[&str]) -> Result<String, Error> {
    let out = run(root, Some(index), args, None)?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
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

/// Whether each of `paths` (project-relative) is a file on disk now.
pub(super) fn files_on_disk(root: &str, paths: &[String]) -> Vec<bool> {
    match crate::system::ssh::split(root) {
        Some(at) => crate::system::ssh::fs::sizes(at.host, at.path, paths).map_or_else(
            |_| vec![false; paths.len()],
            |sizes| sizes.iter().map(Option::is_some).collect(),
        ),
        None => paths
            .iter()
            .map(|p| Path::new(root).join(p).is_file())
            .collect(),
    }
}

/// Deletes the file `rel` of the project at `root`, which a turn created.
fn remove_added(root: &str, rel: &str) -> Result<(), Error> {
    if let Some(at) = crate::system::ssh::split(root) {
        return crate::system::ssh::fs::remove_inside(at.host, at.path, rel);
    }
    let path = Path::new(root).join(rel);
    std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()).into())
}

/// Puts the project's files back as they were before the turn for `message_id`.
///
/// Files changed or deleted since are rewritten, and files created since are deleted. Ignored
/// files are left alone, judged by the ignore rules as they were at the checkpoint, so a folder
/// ignored then and not since stays. The project as it was is kept at `refs/patchwork/pre-restore/<thread>` first.
///
/// # Errors
///
/// Returns an error when the message has no checkpoint, when `git` fails to snapshot or compare the
/// project, or when a file cannot be removed or restored.
pub fn restore(root: &str, thread_id: &str, message_id: &str) -> Result<(), Error> {
    let saved = git(
        root,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &ref_name(thread_id, message_id),
        ],
    )
    .map_err(|_discarded| "This message has no checkpoint to restore")?;
    let now = snapshot(root)?;
    git(
        root,
        &[
            "update-ref",
            &format!("{PRE_RESTORE_REFS}/{thread_id}"),
            &now,
        ],
    )
    .map_err(|e| format!("Could not keep the files as they are before restoring: {e}"))?;
    let diff = run(
        root,
        None,
        &[
            "diff-tree",
            "-r",
            "-z",
            "--relative",
            "--no-renames",
            "--name-status",
            &saved,
            &now,
        ],
        None,
    )?;
    let fields: Vec<&[u8]> = diff.stdout.split(|&b| b == 0).collect();
    let mut rewrite = Vec::new();
    let mut added = Vec::new();
    for pair in fields.chunks(2) {
        let [status, path] = pair else { continue };
        if status == b"A" {
            added.push(String::from_utf8_lossy(path).into_owned());
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
    if added.is_empty() {
        return Ok(());
    }
    // Rewriting put the checkpoint's ignore rules back; a file they ignore was never in it.
    let names = added.join("\n") + "\n";
    let ignored = run(
        root,
        None,
        &["check-ignore", "--no-index", "--stdin"],
        Some(names.as_bytes()),
    )
    .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
    .unwrap_or_default();
    let ignored: std::collections::HashSet<&str> = ignored.lines().collect();
    for path in added.iter().filter(|p| !ignored.contains(p.as_str())) {
        remove_added(root, path)?;
    }
    Ok(())
}

/// Reverses one turn's changes to `files`, leaving the rest of the project untouched.
///
/// Takes back what one turn did to `files`, leaving the rest of the project as it is: the turn's
/// own diff of them is applied to the files on disk in reverse. When a file changed since in a
/// way the reverse no longer fits, nothing is touched.
///
/// # Errors
///
/// Returns an error when the reply changed no files, when `git` fails, or when the reply's changes
/// were edited since and no longer reverse cleanly.
pub fn undo(root: &str, turn: &TurnTrees, files: &[String]) -> Result<(), Error> {
    // Without paths the diff would cover the whole project.
    if files.is_empty() {
        return Err("This reply changed no files".into());
    }
    let mut args = vec![
        "diff",
        "--binary",
        "--no-color",
        "--no-ext-diff",
        &turn.before,
        &turn.after,
        "--",
    ];
    args.extend(files.iter().map(String::as_str));
    let patch = run(root, None, &args, None)?.stdout;
    if patch.is_empty() {
        return Err("This reply changed no files".into());
    }
    // The patch names files from the repository's top, which the project may sit below.
    let top = git(root, &["rev-parse", "--show-toplevel"])?;
    let top = crate::system::ssh::same_host(root, top.trim());
    run(
        &top,
        None,
        &["apply", "--reverse", "--check", "-"],
        Some(&patch),
    )
    .map_err(|e| {
        format!(
            "The reply's changes were edited since, so they can't be taken back cleanly. {}",
            e.to_string()
                .trim_start_matches("git apply --reverse --check -: ")
        )
    })?;
    run(&top, None, &["apply", "--reverse", "-"], Some(&patch))?;
    Ok(())
}

/// Drops the snapshots of the given messages, or of the whole thread when `message_ids` is None.
pub fn remove(root: &str, thread_id: &str, message_ids: Option<&[String]>) {
    let ids = match message_ids {
        Some(ids) => ids.to_vec(),
        None => list(root, thread_id),
    };
    for id in ids {
        delete_ref(root, &ref_name(thread_id, &id));
        delete_ref(root, &end_ref(thread_id, &id));
    }
    // Replies have end snapshots but no snapshot of their own, so they are not in `list`.
    if message_ids.is_none() {
        let ends = git(
            root,
            &[
                "for-each-ref",
                "--format=%(refname)",
                &format!("{END_REFS}/{thread_id}/"),
            ],
        );
        for name in ends.unwrap_or_default().lines() {
            delete_ref(root, name);
        }
        delete_ref(root, &format!("{PRE_RESTORE_REFS}/{thread_id}"));
    }
}

/// Deletes the ref `name`, logging a failure.
fn delete_ref(root: &str, name: &str) {
    if let Err(e) = git(root, &["update-ref", "-d", name]) {
        log::warn!(error:% = e; "failed to drop a snapshot: {{error}}");
    }
}

impl Core {
    /// The folder a thread that belongs to a project works in: its worktree or the project's.
    fn checkpoint_root(&self, thread_id: &str) -> Option<String> {
        self.store.thread(thread_id)?.project_id?;
        self.thread_root(thread_id).ok()
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
    /// leave the thread. Claude picks its session up next turn from the reply now last, which
    /// drops the undone turns and keeps the rest cached. Other agents, and Claude when that reply
    /// has no cursor, start a new session given the remaining messages as a transcript, since
    /// their own session still holds the undone turns.
    ///
    /// # Errors
    ///
    /// Returns an error when a turn is running, when the chat is not a project task or is unknown,
    /// when the message is not found, or when restoring the checkpoint or saving the chat fails.
    pub fn restore_checkpoint(&self, thread_id: &str, message_id: &str) -> Result<Thread, Error> {
        if self.runner.is_running(thread_id) || self.runner.is_starting(thread_id) {
            return Err("Stop the running turn before restoring a checkpoint".into());
        }
        // Its task and subagents share the worktree whose files are restored.
        let _step = self.hold_turn_folder(
            thread_id,
            "Stop the task and its subagents before restoring a checkpoint",
        )?;
        self.store
            .thread(thread_id)
            .and_then(|t| t.project_id)
            .ok_or("Only project tasks have checkpoints")?;
        // Not the project's own folder for a task whose worktree isn't there now.
        let root = self.thread_root(thread_id)?;
        // Another task editing the same folder, such as one more task in the project's own folder,
        // would lose what it wrote since the checkpoint.
        let other_running = self.store.threads().iter().any(|t| {
            t.id != thread_id
                && (self.runner.is_running(&t.id) || self.runner.is_starting(&t.id))
                && self.thread_root(&t.id).is_ok_and(|r| r == root)
        });
        if other_running {
            return Err("Another task is working in this folder. Stop it before restoring".into());
        }
        // An idle agent still holds the session being undone.
        self.runner.retire_idle(thread_id);
        let thread = self.thread(thread_id)?;
        let start = thread
            .messages
            .iter()
            .position(|m| m.id == message_id)
            .ok_or("Message not found")?;
        restore(&root, thread_id, message_id)?;
        let undone: Vec<String> = thread.messages[start..]
            .iter()
            .map(|m| m.id.clone())
            .collect();
        remove(&root, thread_id, Some(&undone));
        let kept = start
            .checked_sub(1)
            .and_then(|last| thread.messages[last].cursor.clone())
            .filter(|_| thread.agent == AgentKind::Claude && thread.session_id.is_some());
        let point = kept.map(|at| ResumePoint { at, fork: false });
        let resumes = point.is_some();
        self.set_resume_point(thread_id, point.as_ref())?;
        self.keep_reply_costs(thread_id, start);
        self.store.update_thread(thread_id, |t| {
            t.messages.truncate(start);
            if !resumes {
                t.session_id = None;
            }
            t.goal = None;
            t.context_pending = !t.messages.is_empty();
        })?;
        self.thread(thread_id)
    }

    /// Takes back the files one reply changed, as `undo` does, while the thread and its other
    /// replies stay as they are. The changes it undoes show up as uncommitted ones.
    ///
    /// # Errors
    ///
    /// Returns an error when a turn is running, when the chat is not a project task or is unknown,
    /// when the reply is not found or has no snapshot, or when `git` cannot reverse its changes.
    pub fn undo_reply(&self, thread_id: &str, message_id: &str) -> Result<(), Error> {
        if self.runner.is_running(thread_id) || self.runner.is_starting(thread_id) {
            return Err("Stop the running turn before undoing a reply".into());
        }
        let _step = self.hold_turn_folder(
            thread_id,
            "Stop the task and its subagents before undoing a reply",
        )?;
        self.store
            .thread(thread_id)
            .and_then(|t| t.project_id)
            .ok_or("Only project tasks have replies to undo")?;
        // Not the project's own folder for a task whose worktree isn't there now.
        let root = self.thread_root(thread_id)?;
        let thread = self.thread(thread_id)?;
        let reply = thread
            .messages
            .iter()
            .find(|m| m.id == message_id)
            .ok_or("Reply not found")?;
        let turn = reply
            .turn
            .as_ref()
            .ok_or("This reply has no snapshot to undo it from")?;
        undo(&root, turn, &reply.files)
    }

    /// Drops the thread's checkpoints, once its messages are gone.
    pub(crate) fn remove_checkpoints(&self, thread_id: &str) {
        // Its snapshots are refs of the repository, which the project's folder reaches too when
        // the task's worktree isn't there; only deleting them happens there.
        let root = self.checkpoint_root(thread_id).or_else(|| {
            let project = self.store.thread(thread_id)?.project_id?;
            self.project_root(&project).ok()
        });
        if let Some(root) = root {
            remove(&root, thread_id, None);
        }
    }
}
