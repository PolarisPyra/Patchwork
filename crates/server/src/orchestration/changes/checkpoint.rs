//! Snapshots of a project's files, taken as each turn starts and ends, and restoring one.
//!
//! A snapshot is a git tree of every file git does not ignore, built in a temporary index so the
//! user's own index and branches are untouched, and kept alive by a ref under `refs/patchwork`. The
//! pair around a turn is what the turn changed, whatever happens to the files later.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::process::Output;

use serde::Serialize;

use crate::Server;
use crate::system::new_id;
use crate::provider::AgentKind;
use crate::threads::{ResumePoint, Thread, TurnTrees};

const REFS: &str = "refs/patchwork/checkpoints";
/// Snapshots from the end of a turn, one ref per reply the turn wrote.
const END_REFS: &str = "refs/patchwork/turn-ends";

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
fn run(
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
    let mut child = crate::system::remote::prepare(cmd)?
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

fn git(root: &str, args: &[&str]) -> Result<String, Error> {
    let out = run(root, None, args, None)?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A tree of the project's files as they are now. The user's index is copied first, so git only
/// re-reads the files that changed since it was last refreshed.
fn snapshot(root: &str) -> Result<String, Error> {
    if crate::system::remote::is_remote(root) {
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
    let Some(at) = crate::system::remote::split(root) else {
        return Err("Not a remote project".into());
    };
    crate::system::remote::op::snapshot(at.host, at.path)
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
        let _ = git(root, &["update-ref", &end_ref(thread_id, id), &after]);
    }
    Some(TurnTrees { before, after })
}

/// Where a file a turn changed stands now.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnFileState {
    /// Still differs from the last commit.
    Uncommitted,
    /// Matches the last commit.
    Committed,
    /// Back to how it was before the turn.
    Reverted,
    /// Deleted by the turn, or since.
    Deleted,
    /// The turn left it as it was.
    Unchanged,
}

/// A file a turn changed: lines added and removed by the turn, and where it stands now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnFile {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
    pub state: TurnFileState,
}

/// Fields of `git diff-tree -z` output for the turn's changes to `paths`.
fn turn_diff_fields(
    root: &str,
    turn: &TurnTrees,
    format: &str,
    paths: &[String],
) -> Result<Vec<String>, Error> {
    let mut args = vec![
        "diff-tree",
        "-r",
        "-z",
        "--relative",
        "--no-renames",
        format,
        &turn.before,
        &turn.after,
        "--",
    ];
    args.extend(paths.iter().map(String::as_str));
    let out = run(root, None, &args, None)?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|f| !f.is_empty())
        .map(String::from)
        .collect())
}

/// What the turn did to each of `paths` (project-relative), and where each file stands now.
///
/// # Errors
///
/// Returns an error when a `git` command that compares the turn's snapshots fails.
pub fn turn_files(root: &str, turn: &TurnTrees, paths: &[String]) -> Result<Vec<TurnFile>, Error> {
    // `A\0path\0` pairs, and `added\tdeleted\tpath\0` lines (`-` for binary files).
    let status: Vec<String> = turn_diff_fields(root, turn, "--name-status", paths)?;
    let status: std::collections::HashMap<&str, &str> = status
        .chunks(2)
        .filter_map(|p| Some((p.get(1)?.as_str(), p[0].as_str())))
        .collect();
    let mut counts = std::collections::HashMap::new();
    for line in turn_diff_fields(root, turn, "--numstat", paths)? {
        let mut parts = line.splitn(3, '\t');
        let (Some(add), Some(del), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        counts.insert(
            path.to_string(),
            (add.parse().unwrap_or(0), del.parse().unwrap_or(0)),
        );
    }
    // The blobs to compare: before the turn, at the last commit, and on disk now.
    let on_disk = files_on_disk(root, paths);
    let present: Vec<&String> = paths
        .iter()
        .zip(&on_disk)
        .filter(|(p, there)| **there && status.get(p.as_str()).is_some_and(|s| *s != "D"))
        .map(|(p, _)| p)
        .collect();
    // Both read paths from the repository's top, which the project may sit below.
    let prefix = git(root, &["rev-parse", "--show-prefix"])?;
    let queries: String = present
        .iter()
        .flat_map(|p| {
            [
                format!("{}:{prefix}{p}\n", turn.before),
                format!("HEAD:{prefix}{p}\n"),
            ]
        })
        .collect();
    let known = run(
        root,
        None,
        &["cat-file", "--batch-check=%(objectname)"],
        Some(queries.as_bytes()),
    )?;
    let known = String::from_utf8_lossy(&known.stdout).into_owned();
    let mut known = known.lines();
    let names: String = present.iter().fold(String::new(), |mut names, p| {
        // Relative to the project, where git runs, so the same names work on an SSH host.
        let _ = writeln!(names, "{p}");
        names
    });
    let now = run(
        root,
        None,
        &["hash-object", "--stdin-paths"],
        Some(names.as_bytes()),
    )?;
    let now = String::from_utf8_lossy(&now.stdout).into_owned();
    let mut blobs = std::collections::HashMap::new();
    for (path, now) in present.iter().zip(now.lines()) {
        let (before, head) = (
            known.next().unwrap_or_default(),
            known.next().unwrap_or_default(),
        );
        blobs.insert(
            path.as_str(),
            (before.to_string(), head.to_string(), now.to_string()),
        );
    }
    Ok(paths
        .iter()
        .map(|path| {
            let (additions, deletions) = counts.get(path).copied().unwrap_or((0, 0));
            let state = match (status.get(path.as_str()), blobs.get(path.as_str())) {
                (None, _) => TurnFileState::Unchanged,
                (_, None) => TurnFileState::Deleted,
                (_, Some((before, _, now))) if before == now => TurnFileState::Reverted,
                (_, Some((_, head, now))) if head == now => TurnFileState::Committed,
                _ => TurnFileState::Uncommitted,
            };
            TurnFile {
                path: path.clone(),
                additions,
                deletions,
                state,
            }
        })
        .collect())
}

/// The turn's diff of one project-relative file, in `git diff` form.
///
/// # Errors
///
/// Returns an error when `git diff` between the turn's snapshots fails.
pub fn turn_file_diff(root: &str, turn: &TurnTrees, path: &str) -> Result<String, Error> {
    git(
        root,
        &[
            "diff",
            "--no-color",
            "--relative",
            &turn.before,
            &turn.after,
            "--",
            path,
        ],
    )
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
fn files_on_disk(root: &str, paths: &[String]) -> Vec<bool> {
    match crate::system::remote::split(root) {
        Some(at) => crate::system::remote::fs::sizes(at.host, at.path, paths).map_or_else(
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
    if let Some(at) = crate::system::remote::split(root) {
        return crate::system::remote::fs::remove_inside(at.host, at.path, rel);
    }
    let path = Path::new(root).join(rel);
    std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()).into())
}

/// Puts the project's files back as they were before the turn for `message_id`: files changed or
/// deleted since are rewritten, and files created since are deleted. Ignored files are left alone.
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
    for pair in fields.chunks(2) {
        let [status, path] = pair else { continue };
        if status == b"A" {
            remove_added(root, &String::from_utf8_lossy(path))?;
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
    let top = crate::system::remote::same_host(root, top.trim());
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
        let _ = git(root, &["update-ref", "-d", &ref_name(thread_id, &id)]);
        let _ = git(root, &["update-ref", "-d", &end_ref(thread_id, &id)]);
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
            let _ = git(root, &["update-ref", "-d", name]);
        }
    }
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
        let root = self
            .checkpoint_root(thread_id)
            .ok_or("Only project tasks have replies to undo")?;
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
        if let Some(root) = self.checkpoint_root(thread_id) {
            remove(&root, thread_id, None);
        }
    }
}
