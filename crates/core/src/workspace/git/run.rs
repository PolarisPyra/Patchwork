//! Running `git` in a project folder, and the smallest questions asked of it.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::process::{Output, Stdio};
use std::sync::{Mutex, PoisonError, mpsc};
use std::time::Duration;

/// How long a git command that talks to a remote may run before it is stopped.
pub(in crate::workspace) const NETWORK_LIMIT: Duration = Duration::from_secs(600);

/// How long any other git command may run before it is stopped, so a stuck lock or a hung
/// filesystem fails the action instead of holding it forever.
const LOCAL_LIMIT: Duration = Duration::from_secs(120);

/// Runs `git <args>` in `root` and returns its raw output.
pub(in crate::workspace) fn git_raw(root: &str, args: &[&str]) -> Result<Output, Error> {
    git_output(root, args).map_err(git_failed)
}

/// `git_raw` with the error from starting git as it came, so a caller can tell git is missing.
/// A command still running at its time limit is killed with what it started, and fails as
/// timed out.
pub(in crate::workspace) fn git_output(root: &str, args: &[&str]) -> std::io::Result<Output> {
    let mut cmd = crate::system::host_command("git");
    cmd.args(args)
        .current_dir(root)
        // Reads such as `git status` would otherwise rewrite the index, which the project
        // watcher would report as a change.
        .env("GIT_OPTIONAL_LOCKS", "0")
        // There is no terminal to type a password in, so a push fails instead of waiting.
        .env("GIT_TERMINAL_PROMPT", "0")
        // Git's messages in English, which the checks of its errors read, such as "not a git
        // repository"; file names keep the user's own character set.
        .env_remove("LC_ALL")
        .env("LC_MESSAGES", "C");
    let cmd = crate::system::ssh::prepare(cmd).map_err(std::io::Error::other)?;
    let what = format!("git {}", subcommand(args).unwrap_or_default());
    output_within(cmd, limit(args), &what)
}

/// Runs `cmd` without input and returns its output. A command still running after `limit` is
/// killed with what it started, and fails as timed out, naming it as `what`.
pub(in crate::workspace) fn output_within(
    mut cmd: std::process::Command,
    limit: Duration,
    what: &str,
) -> std::io::Result<Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // A group of its own, so stopping it also stops the helpers it started.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let child = cmd.spawn()?;
    let pid = child.id();
    let _running = Running::start(pid);
    let (send, done) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(child.wait_with_output());
    });
    if let Ok(out) = done.recv_timeout(limit) {
        return out;
    }
    crate::system::processes::signal_process_group(pid, crate::system::processes::SIGKILL);
    // Its output is read to the end once it is killed; the result says only that it timed out.
    let _ = done.recv();
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!(
            "{what} was still running after {} s and was stopped",
            limit.as_secs()
        ),
    ))
}

/// The pids of the commands [`output_within`] is running now.
static RUNNING: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// The pids of the git and `gh` commands Patchwork is running now, each the leader of the
/// process group its helpers are in.
pub(crate) fn own_commands() -> Vec<u32> {
    RUNNING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// A command in [`RUNNING`] until it drops.
struct Running(u32);

impl Running {
    fn start(pid: u32) -> Self {
        RUNNING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(pid);
        Self(pid)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|pid| *pid != self.0);
    }
}

/// The git command in `args`, past the options before it, such as `-c name=value`.
fn subcommand<'a>(args: &[&'a str]) -> Option<&'a str> {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match *arg {
            "-c" | "-C" => {
                args.next();
            }
            a if a.starts_with('-') => {}
            a => return Some(a),
        }
    }
    None
}

/// How long the git command in `args` may run.
fn limit(args: &[&str]) -> Duration {
    match subcommand(args) {
        Some("push" | "pull" | "fetch" | "clone" | "ls-remote" | "submodule") => NETWORK_LIMIT,
        _ => LOCAL_LIMIT,
    }
}

/// The error for git that could not be run, or that ran out of time.
pub(in crate::workspace) fn git_failed(e: std::io::Error) -> Error {
    // Its message already names the command, and the kind lets a caller offer to try again.
    if e.kind() == std::io::ErrorKind::TimedOut {
        return e.into();
    }
    Error::Git(format!("Failed to run git: {e}"))
}

/// Runs `git <args>` in `root` and returns stdout; a non-zero exit becomes an error carrying stderr.
pub(crate) fn git(root: &str, args: &[&str]) -> Result<String, Error> {
    let out = git_raw(root, args)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(Error::Git(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

pub(in crate::workspace) fn git_succeeds(root: &str, args: &[&str]) -> bool {
    git_raw(root, args).is_ok_and(|o| o.status.success())
}

pub(in crate::workspace) fn is_untracked(root: &str, rel: &str) -> bool {
    !git_succeeds(root, &["ls-files", "--error-unmatch", "--", rel])
}

pub(in crate::workspace) fn has_head(root: &str) -> bool {
    git_succeeds(root, &["rev-parse", "--verify", "-q", "HEAD"])
}

#[must_use]
pub fn git_branch(root: &str) -> Option<String> {
    git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
