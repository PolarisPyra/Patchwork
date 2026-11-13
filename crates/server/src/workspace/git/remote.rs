//! The remote side of a repository: sync state, pushing, and pull requests through `gh`.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::process::Stdio;

use crate::workspace::{GitSyncState, PullRequest};

use super::commits::truncated;
use super::run::{git, git_branch, git_succeeds, has_head};

/// The remote to push to: `origin`, else the first one. None in a repository without remotes.
fn push_remote(root: &str) -> Option<String> {
    let remotes = git(root, &["remote"]).ok()?;
    let remotes: Vec<&str> = remotes
        .lines()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .collect();
    remotes
        .iter()
        .find(|&&r| r == "origin")
        .or(remotes.first())
        .map(std::string::ToString::to_string)
}

/// The branch pull requests go into: the remote's HEAD, else `main` or `master`.
fn default_branch(root: &str, remote: Option<&str>) -> Option<String> {
    if let Some(remote) = remote
        && let Ok(head) = git(
            root,
            &[
                "symbolic-ref",
                "--short",
                &format!("refs/remotes/{remote}/HEAD"),
            ],
        )
        && let Some(branch) = head.trim().strip_prefix(&format!("{remote}/"))
    {
        return Some(branch.to_string());
    }
    ["main", "master"]
        .into_iter()
        .map(String::from)
        .find(|name| {
            let remote_ref = remote.map(|r| format!("refs/remotes/{r}/{name}"));
            remote_ref.is_some_and(|r| git_succeeds(root, &["show-ref", "--verify", "-q", &r]))
                || git_succeeds(
                    root,
                    &["show-ref", "--verify", "-q", &format!("refs/heads/{name}")],
                )
        })
}

/// The default branch to compare the checked-out one with: its remote branch when that was
/// fetched, else the local one. None when there is no telling which it is.
pub(crate) fn base_ref(root: &str) -> Option<String> {
    let remote = push_remote(root);
    let base = default_branch(root, remote.as_deref())?;
    Some(
        remote
            .map(|r| format!("{r}/{base}"))
            .filter(|r| git_succeeds(root, &["rev-parse", "--verify", "-q", r]))
            .unwrap_or(base),
    )
}

fn rev_count(root: &str, args: &[&str]) -> u32 {
    let mut full = vec!["rev-list", "--count"];
    full.extend(args);
    git(root, &full)
        .ok()
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0)
}

/// Where the branch stands against its remote. Asks `gh` for an open pull request only on a
/// pushed branch other than the default one, since that call goes over the network.
#[must_use]
pub fn git_sync_state(root: &str) -> GitSyncState {
    let Some(branch) = git_branch(root).filter(|b| b != "HEAD") else {
        return GitSyncState::default();
    };
    let remote = push_remote(root);
    let default_branch = default_branch(root, remote.as_deref());
    let has_upstream = git_succeeds(root, &["rev-parse", "--verify", "-q", "@{u}"]);
    let (behind, ahead) = if has_upstream {
        let counts = git(
            root,
            &["rev-list", "--left-right", "--count", "@{u}...HEAD"],
        )
        .unwrap_or_default();
        let mut counts = counts.split_whitespace().map(|n| n.parse().unwrap_or(0));
        (counts.next().unwrap_or(0), counts.next().unwrap_or(0))
    } else if has_head(root) {
        (0, rev_count(root, &["HEAD", "--not", "--remotes"]))
    } else {
        (0, 0)
    };
    let on_default = default_branch.as_deref() == Some(branch.as_str());
    let pull_request = (has_upstream && !on_default)
        .then(|| open_pull_request(root))
        .flatten();
    GitSyncState {
        has_changes: git(root, &["status", "--porcelain"]).is_ok_and(|s| !s.trim().is_empty()),
        has_remote: remote.is_some(),
        has_upstream,
        ahead,
        behind,
        pull_request,
        default_branch,
        branch: Some(branch),
    }
}

/// Runs `gh <args>` in `root`, without prompts. A missing `gh` is said plainly.
pub(crate) fn gh(root: &str, args: &[&str]) -> Result<String, Error> {
    let mut cmd = crate::system::host_command("gh");
    cmd.args(args)
        .current_dir(root)
        .env("GH_PROMPT_DISABLED", "1");
    let out = crate::system::remote::prepare(cmd)?
        .stdin(Stdio::null())
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::Message(
                "Pull requests need the GitHub CLI: install gh and run gh auth login".into(),
            ),
            _ => Error::Message(format!("Failed to run gh: {e}")),
        })?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(Error::Message(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

/// Where one of a pull request's checks stands.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckState {
    Failed,
    Pending,
    Passed,
}

/// Reads one entry of `gh pr view`'s `statusCheckRollup`: a GitHub Actions run, or a status
/// another service set. Skipped and neutral runs count as passed.
pub(crate) fn check_state(check: &serde_json::Value) -> CheckState {
    let failed = |s: &str| {
        matches!(
            s,
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE"
        )
    };
    if let Some(state) = check["state"].as_str() {
        return match state {
            "PENDING" | "EXPECTED" => CheckState::Pending,
            s if failed(s) => CheckState::Failed,
            _ => CheckState::Passed,
        };
    }
    if check["status"].as_str() != Some("COMPLETED") {
        CheckState::Pending
    } else if check["conclusion"].as_str().is_some_and(failed) {
        CheckState::Failed
    } else {
        CheckState::Passed
    }
}

/// The branch's open pull request, if `gh` can tell, with where its checks stand and how many
/// comments and reviews it has.
fn open_pull_request(root: &str) -> Option<PullRequest> {
    let out = gh(
        root,
        &[
            "pr",
            "view",
            "--json",
            "number,url,state,statusCheckRollup,comments,reviews",
        ],
    )
    .ok()?;
    let value: serde_json::Value = serde_json::from_str(&out).ok()?;
    if value["state"] != "OPEN" {
        return None;
    }
    let states: Vec<CheckState> = value["statusCheckRollup"]
        .as_array()
        .into_iter()
        .flatten()
        .map(check_state)
        .collect();
    let count = |state| states.iter().filter(|s| **s == state).count() as u32;
    let len = |key: &str| value[key].as_array().map_or(0, Vec::len) as u32;
    Some(PullRequest {
        number: value["number"].as_u64()? as u32,
        url: value["url"].as_str()?.to_string(),
        failed: count(CheckState::Failed),
        pending: count(CheckState::Pending),
        passed: count(CheckState::Passed),
        comments: len("comments") + len("reviews"),
    })
}

/// Pushes the branch. Its first push sets the upstream on `origin`, or the first remote.
/// Returns where it went, such as `origin/main`.
///
/// # Errors
///
/// Returns an error when no branch is checked out, the repository has no remote to push to, or `git
/// push` fails.
pub fn push(root: &str) -> Result<String, Error> {
    let branch = git_branch(root)
        .filter(|b| b != "HEAD")
        .ok_or_else(|| Error::Message("Check out a branch to push".into()))?;
    if git_succeeds(root, &["rev-parse", "--verify", "-q", "@{u}"]) {
        git(root, &["push"])?;
        return Ok(git(root, &["rev-parse", "--abbrev-ref", "@{u}"])?
            .trim()
            .to_string());
    }
    let remote = push_remote(root)
        .ok_or_else(|| Error::Message("This repository has no remote to push to".into()))?;
    git(root, &["push", "-u", &remote, "HEAD"])?;
    Ok(format!("{remote}/{branch}"))
}

/// What pull request text is written from: the branches, the commits since the default branch,
/// and their diff, cut to fit one command line on every system. Also returns the base.
pub(crate) fn branch_summary(root: &str) -> Result<(String, String), Error> {
    let branch = git_branch(root).unwrap_or_default();
    let remote = push_remote(root);
    let base = default_branch(root, remote.as_deref()).ok_or_else(|| {
        Error::Message("Can't tell which branch the pull request goes into".into())
    })?;
    if base == branch {
        return Err(Error::Message(format!(
            "{branch} is the default branch: open a pull request from another branch"
        )));
    }
    let base_ref = remote
        .map(|r| format!("{r}/{base}"))
        .filter(|r| git_succeeds(root, &["rev-parse", "--verify", "-q", r]))
        .unwrap_or_else(|| base.clone());
    let range = format!("{base_ref}...HEAD");
    let commits = git(
        root,
        &[
            "log",
            "--oneline",
            "--no-decorate",
            &format!("{base_ref}..HEAD"),
        ],
    )?;
    let stat = git(root, &["diff", "--stat", &range])?;
    let patch = git(
        root,
        &["diff", "--no-color", "--no-ext-diff", "--minimal", &range],
    )?;
    let summary = format!(
        "Base branch: {base}\nHead branch: {branch}\n\nCommits:\n{}\n\nDiff stat:\n{}\n\nDiff patch:\n{}",
        truncated(&commits, 4_000),
        truncated(&stat, 4_000),
        truncated(&patch, 12_000)
    );
    Ok((base, summary))
}

/// Opens a pull request for the pushed branch into `base` with `gh`, and returns it. Without a
/// `title`, gh fills the title and body in from the commits. An open one is returned as it is.
pub(crate) fn create_pull_request(
    root: &str,
    base: &str,
    text: Option<(&str, &str)>,
) -> Result<PullRequest, Error> {
    if let Some(existing) = open_pull_request(root) {
        return Ok(existing);
    }
    let mut args = vec!["pr", "create", "--base", base];
    match text {
        Some((title, body)) => args.extend(["--title", title, "--body", body]),
        None => args.push("--fill"),
    }
    let out = gh(root, &args)?;
    let url = out
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| l.starts_with("http"))
        .ok_or_else(|| Error::Message(format!("gh opened no pull request: {}", out.trim())))?;
    Ok(PullRequest {
        number: url
            .rsplit('/')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0),
        url: url.to_string(),
        // Nothing has run or been said on a pull request just opened.
        failed: 0,
        pending: 0,
        passed: 0,
        comments: 0,
    })
}
