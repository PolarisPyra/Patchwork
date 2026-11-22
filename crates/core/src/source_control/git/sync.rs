//! Trading commits with a remote: pushing, pulling, fetching, and both ways at once.

// Rust guideline compliant 2026-10-09

use crate::Error;

use super::operation::{none_stopped, stopped};
use super::askpass::Attended;
use super::remote::unattended_ssh;
use super::run::{explained, git, git_branch, git_succeeds};

/// The remote a branch is published to when the repository has it.
const ORIGIN: &str = "origin";

/// The repository's remotes that can be pushed to, `origin` first, then in git's order.
pub(in crate::source_control) fn push_remotes(root: &str) -> Vec<String> {
    let mut remotes: Vec<String> = Vec::new();
    for line in git(root, &["remote", "-v"]).unwrap_or_default().lines() {
        let Some((name, rest)) = line.split_once('\t') else {
            continue;
        };
        if rest.trim_end().ends_with("(push)") && !remotes.iter().any(|r| r == name) {
            remotes.push(name.to_string());
        }
    }
    if let Some(at) = remotes.iter().position(|r| r == ORIGIN) {
        remotes[..=at].rotate_right(1);
    }
    remotes
}

/// A setting of the checked-out branch, such as `remote` or `merge`.
fn branch_config(root: &str, branch: &str, key: &str) -> Result<String, Error> {
    git(
        root,
        &["config", "--get", &format!("branch.{branch}.{key}")],
    )
    .map(|v| v.trim().to_string())
}

fn has_upstream(root: &str) -> bool {
    git_succeeds(root, &["rev-parse", "--verify", "-q", "@{u}"])
}

/// Pushes the branch to its upstream, or to `remote` when one is given or it has no upstream yet.
///
/// Its first push sets the upstream, on `origin` or the repository's only remote. With `force`
/// the remote's branch is replaced, unless it moved since it was last fetched. Returns where it
/// went, such as `origin/main`.
///
/// # Errors
///
/// Returns an error when no branch is checked out, there is no telling which remote to push to,
/// or `git push` fails.
pub fn push(root: &str, remote: Option<&str>, force: bool) -> Result<String, Error> {
    let _asking = Attended::start();
    let branch = git_branch(root)
        .filter(|b| b != "HEAD")
        .ok_or_else(|| Error::Message("Check out a branch to push".into()))?;
    let tracked = has_upstream(root);
    let mut args = unattended_ssh(root);
    args.push("push");
    if force {
        args.push("--force-with-lease");
    }
    if let (None, true) = (remote, tracked) {
        // Named in full, so an upstream with another name than the branch, as an agent's
        // `git push -u origin HEAD:<name>` sets, takes the push that `push.default` would refuse.
        let remote = branch_config(root, &branch, "remote")?;
        let to = format!("HEAD:{}", branch_config(root, &branch, "merge")?);
        args.extend([remote.as_str(), to.as_str()]);
        git(root, &args).map_err(explained)?;
        return Ok(git(root, &["rev-parse", "--abbrev-ref", "@{u}"])?
            .trim()
            .to_string());
    }
    let remote = if let Some(remote) = remote {
        remote.to_string()
    } else {
        let remotes = push_remotes(root);
        match remotes.as_slice() {
            [] => {
                return Err(Error::Message(
                    "This repository has no remote to push to".into(),
                ));
            }
            [first, rest @ ..] if first == ORIGIN || rest.is_empty() => first.clone(),
            _ => {
                return Err(Error::Message(
                    "This repository has several remotes. Pick the one to publish to.".into(),
                ));
            }
        }
    };
    let to = format!("HEAD:refs/heads/{branch}");
    if !tracked {
        args.push("-u");
    }
    args.extend([remote.as_str(), to.as_str()]);
    git(root, &args).map_err(explained)?;
    Ok(format!("{remote}/{branch}"))
}

/// Pushes every tag to the branch's remote, or the first one, and returns the remote.
///
/// # Errors
///
/// Returns an error when the repository has no remote or `git push` fails.
pub fn push_tags(root: &str) -> Result<String, Error> {
    let _asking = Attended::start();
    let remote = branch_remote(root)
        .ok_or_else(|| Error::Message("This repository has no remote to push to".into()))?;
    let mut args = unattended_ssh(root);
    args.extend(["push", &remote, "--tags"]);
    git(root, &args).map_err(explained)?;
    Ok(remote)
}

/// The remote the checked-out branch tracks, else the one it would be published to.
fn branch_remote(root: &str) -> Option<String> {
    git_branch(root)
        .and_then(|branch| branch_config(root, &branch, "remote").ok())
        .filter(|remote| !remote.is_empty() && remote != ".")
        .or_else(|| push_remotes(root).into_iter().next())
}

/// Brings the upstream's new commits into the branch, setting uncommitted changes aside
/// meanwhile.
///
/// The branch's own commits are replayed on top, unless the repository's settings say how it
/// pulls (`pull.rebase`, `pull.ff`, or the branch's own `rebase`), which are then followed. A
/// pull that conflicts stays stopped, to resolve and continue or to abort. Returns the upstream,
/// such as `origin/main`.
///
/// # Errors
///
/// Returns an error when the branch has no upstream, an operation is already stopped, or `git
/// pull` fails or stops on conflicts.
pub fn pull(root: &str) -> Result<String, Error> {
    let _asking = Attended::start();
    let upstream = git(root, &["rev-parse", "--abbrev-ref", "@{u}"]).map_err(|e| {
        log::info!(error:% = e; "found no upstream to pull from: {{error}}");
        Error::Message("This branch has no upstream to pull from".into())
    })?;
    none_stopped(root)?;
    let branch = git_branch(root).unwrap_or_default();
    let own = format!("branch.{branch}.rebase");
    let configured = ["pull.rebase", "pull.ff", &own]
        .iter()
        .any(|key| git_succeeds(root, &["config", "--get", key]));
    let mut args = unattended_ssh(root);
    args.extend(["pull", "--quiet", "--autostash"]);
    if !configured {
        args.push("--rebase");
    }
    git(root, &args).map_err(|e| stopped(root, "The pull", e))?;
    Ok(upstream.trim().to_string())
}

/// Fetches the branch's remote, or every remote with `all`, with its tags, and forgets the
/// branches deleted there. Returns what was fetched, such as `origin` or "every remote".
///
/// # Errors
///
/// Returns an error when the repository has no remote or `git fetch` fails.
pub fn fetch(root: &str, all: bool) -> Result<String, Error> {
    let _asking = Attended::start();
    let remote = branch_remote(root)
        .ok_or_else(|| Error::Message("This repository has no remote to fetch".into()))?;
    let mut args = unattended_ssh(root);
    args.extend(["fetch", "--quiet", "--prune", "--tags"]);
    if all {
        args.push("--all");
    } else {
        args.extend(["--", &remote]);
    }
    git(root, &args).map_err(explained)?;
    Ok(if all { "every remote".into() } else { remote })
}

/// Pulls the upstream's commits, then pushes the branch's own. A branch without an upstream is
/// published. Returns where it went, such as `origin/main`.
///
/// # Errors
///
/// Returns an error when the pull or the push does.
pub fn sync(root: &str) -> Result<String, Error> {
    if has_upstream(root) {
        pull(root)?;
    }
    push(root, None, false)
}
