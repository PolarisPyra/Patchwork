//! Running `git` in a project folder, and the smallest questions asked of it.

// Rust guideline compliant 2026-10-09

use patchwork_platform::Locked;
use patchwork_platform::Error;
use std::process::{Output, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

/// How long a git command that talks to a remote may run before it is stopped.
pub(crate) const NETWORK_LIMIT: Duration = Duration::from_secs(600);

/// How long any other git command may run before it is stopped, so a stuck lock or a hung
/// filesystem fails the action instead of holding it forever.
const LOCAL_LIMIT: Duration = Duration::from_secs(120);

/// Runs `git <args>` in `root` and returns its raw output.
pub(crate) fn git_raw(root: &str, args: &[&str]) -> Result<Output, Error> {
    git_output(root, args).map_err(git_failed)
}

/// How many times a command refused by another git command's lock is run before it fails.
const LOCK_ATTEMPTS: u64 = 6;

/// `git_raw` with the error from starting git as it came, so a caller can tell git is missing.
/// A command still running at its time limit is killed with what it started, and fails as
/// timed out. One refused because another git command holds the index or a ref is run again a
/// few times, waiting longer each time.
pub(crate) fn git_output(root: &str, args: &[&str]) -> std::io::Result<Output> {
    let what = format!("git {}", subcommand(args).unwrap_or_default());
    let mut attempt = 1;
    loop {
        let mut cmd = patchwork_platform::host_command("git");
        cmd.args(args)
            .current_dir(root)
            // Reads such as `git status` would otherwise rewrite the index, which the project
            // watcher would report as a change.
            .env("GIT_OPTIONAL_LOCKS", "0")
            // A file named like a pattern, such as `a[1].txt` or `*.rs`, names only itself.
            .env("GIT_LITERAL_PATHSPECS", "1")
            // There is no terminal to type a password in, so a push fails instead of waiting.
            .env("GIT_TERMINAL_PROMPT", "0")
            // Git's messages in English, which the checks of its errors read, such as "not a git
            // repository"; file names keep the user's own character set.
            .env_remove("LC_ALL")
            .env("LC_MESSAGES", "C");
        // One the user started asks in the app for what it needs to sign in.
        super::askpass::offer(&mut cmd, root);
        let cmd = patchwork_platform::ssh::prepare(cmd).map_err(std::io::Error::other)?;
        let out = output_within(cmd, limit(args), &what)?;
        let locked = String::from_utf8_lossy(&out.stderr).contains(".lock': File exists");
        if out.status.success() || !locked || attempt == LOCK_ATTEMPTS {
            return Ok(out);
        }
        std::thread::sleep(Duration::from_millis(50 * attempt * attempt));
        attempt += 1;
    }
}

/// Runs `cmd` without input and returns its output. A command still running after `limit` is
/// killed with what it started, and fails as timed out, naming it as `what`.
pub(crate) fn output_within(
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
    patchwork_platform::processes::running::signal_process_group(
        pid,
        patchwork_platform::processes::running::SIGKILL,
    );
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
pub fn own_commands() -> Vec<u32> {
    RUNNING.locked().clone()
}

/// A command in [`RUNNING`] until it drops.
struct Running(u32);

impl Running {
    fn start(pid: u32) -> Self {
        RUNNING.locked().push(pid);
        Self(pid)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.locked().retain(|pid| *pid != self.0);
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

/// How long the git command in `args` may run. A commit runs the repository's hooks, which may
/// lint or test the project, so it gets as long as a remote.
fn limit(args: &[&str]) -> Duration {
    match subcommand(args) {
        Some("push" | "pull" | "fetch" | "clone" | "ls-remote" | "submodule" | "commit") => {
            NETWORK_LIMIT
        }
        _ => LOCAL_LIMIT,
    }
}

/// The error for git that could not be run, or that ran out of time.
pub(crate) fn git_failed(e: std::io::Error) -> Error {
    // Its message already names the command, and the kind lets a caller offer to try again.
    if e.kind() == std::io::ErrorKind::TimedOut {
        return e.into();
    }
    Error::Git(format!("Failed to run git: {e}"))
}

/// Runs `git <args>` in `root` and returns stdout; a non-zero exit becomes an error carrying stderr.
///
/// # Errors
///
/// Returns an error when git cannot be spawned, times out, or exits with a non-zero status.
pub fn git(root: &str, args: &[&str]) -> Result<String, Error> {
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

pub(crate) fn git_succeeds(root: &str, args: &[&str]) -> bool {
    git_raw(root, args).is_ok_and(|o| o.status.success())
}

/// Whether git reports `rel` as untracked. A git that fails is an error, never an untracked file,
/// since an untracked file is deleted to discard it.
pub(crate) fn is_untracked(root: &str, rel: &str) -> Result<bool, Error> {
    let out = git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            rel,
        ],
    )?;
    Ok(out.starts_with("?? "))
}

pub(crate) fn has_head(root: &str) -> bool {
    git_succeeds(root, &["rev-parse", "--verify", "-q", "HEAD"])
}

/// The checked-out branch, also before its first commit, or `HEAD` when none is checked out.
/// None outside a repository.
#[must_use]
pub fn git_branch(root: &str) -> Option<String> {
    match git(root, &["symbolic-ref", "--short", "-q", "HEAD"]) {
        Ok(name) => Some(name.trim().to_string()).filter(|s| !s.is_empty()),
        Err(_) => has_head(root).then(|| "HEAD".to_string()),
    }
}

/// What git says when it fails for a reason the user can act on, and what to tell them instead.
/// The first entry whose text git's output holds wins, so the narrower ones come first.
const PLAIN: &[(&[&str], &str)] = &[
    (
        &[
            "Please tell me who you are",
            "empty ident",
            "auto-detection is disabled",
        ],
        "Git has no name and email to sign commits with. Set user.name and user.email in your git \
         config, then commit again.",
    ),
    (
        &["stale info"],
        "The remote branch moved since it was last fetched. Fetch, look at what changed, then \
         force push again.",
    ),
    (
        &["non-fast-forward", "fetch first", "[rejected]"],
        "The remote has commits this branch lacks. Pull, then push again.",
    ),
    (
        &[
            "Authentication failed",
            "Permission denied",
            "could not read Username",
            "could not read Password",
        ],
        "The remote refused the sign-in. Check your git credentials or SSH key.",
    ),
    (
        &[
            "Could not resolve host",
            "Could not read from remote repository",
            "unable to access",
        ],
        "Couldn't reach the remote. Check the connection and the remote's address.",
    ),
    (
        &["would be overwritten by"],
        "Your uncommitted changes would be overwritten. Commit, stash or discard them first.",
    ),
    (
        &["not fully merged"],
        "The branch has commits no other branch has. Delete it by force to lose them.",
    ),
    (
        &["is not a valid branch name", "is not a valid tag name"],
        "That isn't a name git accepts.",
    ),
    (&["already exists"], "That name is already taken."),
    (&["No local changes to save"], "There is nothing to stash."),
];

/// A failed git command's error in plain words when it is one the user can act on, with what git
/// said kept as its detail. Any other error comes back as it is.
pub(crate) fn explained(e: Error) -> Error {
    let Error::Git(said) = &e else { return e };
    let plain = PLAIN
        .iter()
        .find(|(marks, _)| marks.iter().any(|mark| said.contains(mark)));
    match plain {
        Some((_, message)) => Error::Tool {
            message: (*message).to_string(),
            detail: said.clone(),
        },
        None => e,
    }
}
