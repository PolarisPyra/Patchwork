//! Running `git` in a project folder, and the smallest questions asked of it.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::process::{Output, Stdio};

/// Runs `git <args>` in `root` and returns its raw output.
pub(in crate::workspace) fn git_raw(root: &str, args: &[&str]) -> Result<Output, Error> {
    git_output(root, args).map_err(git_failed)
}

/// `git_raw` with the error from starting git as it came, so a caller can tell git is missing.
pub(in crate::workspace) fn git_output(root: &str, args: &[&str]) -> std::io::Result<Output> {
    let mut cmd = crate::system::host_command("git");
    cmd.args(args)
        .current_dir(root)
        // Reads such as `git status` would otherwise rewrite the index, which the project
        // watcher would report as a change.
        .env("GIT_OPTIONAL_LOCKS", "0")
        // There is no terminal to type a password in, so a push fails instead of waiting.
        .env("GIT_TERMINAL_PROMPT", "0");
    crate::system::remote::prepare(cmd)
        .map_err(std::io::Error::other)?
        .stdin(Stdio::null())
        .output()
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "an adapter for `map_err`, which passes the error by value"
)]
pub(in crate::workspace) fn git_failed(e: std::io::Error) -> Error {
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
