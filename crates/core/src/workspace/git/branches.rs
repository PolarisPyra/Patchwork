//! Branches, tags and remotes: listing them, and making, switching, renaming and deleting them.

// Rust guideline compliant 2026-10-09

use crate::Error;
use crate::workspace::GitBranch;

use super::operation::none_stopped;
use super::remote::base_ref;
use super::run::{explained, git, git_branch, git_succeeds};

/// What a new branch's first reflog entry says before the name it was made from.
const CREATED_FROM: &str = "branch: Created from ";

/// The repository's branches, remote branches and tags, each kind newest first.
///
/// # Errors
///
/// Returns an error when `git for-each-ref` fails.
pub fn git_branches(root: &str) -> Result<Vec<GitBranch>, Error> {
    let out = git(
        root,
        &[
            "for-each-ref",
            "--sort=-creatordate",
            "--format=%(refname)%1f%(HEAD)%1f%(upstream:short)%1f%(subject)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )?;
    let mut refs: Vec<GitBranch> = out
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\x1f');
            let full = fields.next()?;
            let (kind, name) = if let Some(name) = full.strip_prefix("refs/heads/") {
                ("local", name)
            } else if let Some(name) = full.strip_prefix("refs/remotes/") {
                ("remote", name)
            } else {
                ("tag", full.strip_prefix("refs/tags/")?)
            };
            // `origin/HEAD` only repeats the remote's default branch.
            if kind == "remote" && name.ends_with("/HEAD") {
                return None;
            }
            Some(GitBranch {
                name: name.to_string(),
                kind: kind.to_string(),
                current: fields.next()? == "*",
                upstream: fields.next().filter(|u| !u.is_empty()).map(String::from),
                subject: fields.next().unwrap_or_default().to_string(),
            })
        })
        .collect();
    let order = |kind: &str| match kind {
        "local" => 0,
        "remote" => 1,
        _ => 2,
    };
    refs.sort_by_key(|r| order(&r.kind));
    Ok(refs)
}

fn has_ref(root: &str, full: &str) -> bool {
    git_succeeds(root, &["show-ref", "--verify", "-q", full])
}

/// Checks out the branch `name`. A remote branch such as `origin/topic` becomes the local branch
/// `topic` tracking it, and a tag or a commit, or anything with `detached`, is checked out with
/// no branch.
///
/// # Errors
///
/// Returns an error when an operation is stopped, or `git checkout` fails, as when uncommitted
/// changes would be overwritten.
pub fn checkout(root: &str, name: &str, detached: bool) -> Result<(), Error> {
    none_stopped(root)?;
    let local = || has_ref(root, &format!("refs/heads/{name}"));
    let run = |args: &[&str]| git(root, args).map(|_| ()).map_err(explained);
    if detached || (!local() && !has_ref(root, &format!("refs/remotes/{name}"))) {
        return run(&["checkout", "--detach", name, "--"]);
    }
    if local() {
        return run(&["checkout", name, "--"]);
    }
    let short = name.split_once('/').map_or(name, |(_, branch)| branch);
    if has_ref(root, &format!("refs/heads/{short}")) {
        run(&["checkout", short, "--"])
    } else {
        run(&["checkout", "-b", short, "--track", name, "--"])
    }
}

/// Makes the branch `name` at `from`, or at the commit checked out, and checks it out.
///
/// # Errors
///
/// Returns an error when an operation is stopped or `git checkout -b` fails, as for a name taken.
pub fn create_branch(root: &str, name: &str, from: Option<&str>) -> Result<(), Error> {
    none_stopped(root)?;
    let mut args = vec!["checkout", "-b", name];
    args.extend(from);
    args.push("--");
    git(root, &args).map(|_| ()).map_err(explained)
}

/// Deletes the local branch `name`. Without `force` a branch with commits no other has stays.
///
/// # Errors
///
/// Returns an error when `git branch` refuses, as for the branch checked out.
pub fn delete_branch(root: &str, name: &str, force: bool) -> Result<(), Error> {
    let flag = if force { "-D" } else { "-d" };
    git(root, &["branch", flag, "--", name])
        .map(|_| ())
        .map_err(explained)
}

/// Renames the checked-out branch to `name`.
///
/// # Errors
///
/// Returns an error when `git branch -m` fails, as for a name taken.
pub fn rename_branch(root: &str, name: &str) -> Result<(), Error> {
    git(root, &["branch", "-m", "--", name])
        .map(|_| ())
        .map_err(explained)
}

/// Sets the remote branch the checked-out one tracks, such as `origin/main`.
///
/// # Errors
///
/// Returns an error when `git branch` fails, as for a remote branch that isn't fetched.
pub fn set_upstream(root: &str, upstream: &str) -> Result<(), Error> {
    git(root, &["branch", &format!("--set-upstream-to={upstream}")])
        .map(|_| ())
        .map_err(explained)
}

/// Tags the commit `at`, or the one checked out, as `name`, with `message` when it has one.
///
/// # Errors
///
/// Returns an error when `git tag` fails, as for a name taken.
pub fn create_tag(root: &str, name: &str, message: &str, at: Option<&str>) -> Result<(), Error> {
    let mut args = vec!["tag"];
    if !message.trim().is_empty() {
        args.extend(["-a", "-m", message]);
    }
    args.push(name);
    args.extend(at);
    git(root, &args).map(|_| ()).map_err(explained)
}

/// Deletes the tag `name` from this repository. The remote keeps its own.
///
/// # Errors
///
/// Returns an error when `git tag -d` fails.
pub fn delete_tag(root: &str, name: &str) -> Result<(), Error> {
    git(root, &["tag", "-d", "--", name])
        .map(|_| ())
        .map_err(explained)
}

/// Adds the remote `name` at `url`.
///
/// # Errors
///
/// Returns an error when `git remote add` fails, as for a name taken.
pub fn add_remote(root: &str, name: &str, url: &str) -> Result<(), Error> {
    git(root, &["remote", "add", "--", name, url])
        .map(|_| ())
        .map_err(explained)
}

/// Removes the remote `name`, with the branches fetched from it.
///
/// # Errors
///
/// Returns an error when `git remote remove` fails.
pub fn remove_remote(root: &str, name: &str) -> Result<(), Error> {
    git(root, &["remote", "remove", "--", name])
        .map(|_| ())
        .map_err(explained)
}

/// The ref the checked-out branch goes back into, in full: the branch it was made from, by its
/// first reflog entry, as its upstream when it is a local one; else the remote's default branch.
pub(in crate::workspace) fn branch_base(root: &str) -> Option<String> {
    made_from(root).or_else(|| {
        base_ref(root)
            .map(|base| format!("refs/remotes/{base}"))
            .filter(|base| git_succeeds(root, &["rev-parse", "--verify", "-q", base]))
    })
}

/// The branch the checked-out one was made from, as its reflog tells while it still has the
/// entry.
fn made_from(root: &str) -> Option<String> {
    let branch = git_branch(root).filter(|b| b != "HEAD")?;
    let own = format!("refs/heads/{branch}");
    let log = git(root, &["reflog", "show", "--format=%gs", &own]).ok()?;
    let from = log.lines().last()?.strip_prefix(CREATED_FROM)?.trim();
    let from = from.strip_prefix("refs/remotes/").unwrap_or(from);
    let from = from.strip_prefix("refs/heads/").unwrap_or(from);
    let remote = format!("refs/remotes/{from}");
    if has_ref(root, &remote) {
        return Some(remote);
    }
    let local = format!("refs/heads/{from}");
    if local == own || !has_ref(root, &local) {
        return None;
    }
    let tracked = git(
        root,
        &[
            "rev-parse",
            "--symbolic-full-name",
            "-q",
            &format!("{from}@{{u}}"),
        ],
    )
    .ok()
    .map(|u| u.trim().to_string())
    .filter(|u| !u.is_empty());
    Some(tracked.unwrap_or(local))
}
