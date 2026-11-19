//! The remote side of a repository: sync state, pushing, and pull requests through `gh`.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::workspace::{GitSyncState, PullRequest};

use super::commits::truncated;
use super::operation::{operation, prepared_message};
use super::run::{NETWORK_LIMIT, git, git_branch, git_succeeds, has_head, output_within};
use super::sync::push_remotes;

/// The remote to push to: `origin`, else the first one that takes pushes. None in a repository
/// without one.
fn push_remote(root: &str) -> Option<String> {
    push_remotes(root).into_iter().next()
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

/// How long after a fetch of a repository's remote the next one is due.
const FETCH_EVERY: Duration = Duration::from_secs(15);

/// How long after a failed fetch the next try waits. It doubles with each failure in a row, up
/// to [`FETCH_RETRY_MAX`].
const FETCH_RETRY: Duration = Duration::from_secs(30);
const FETCH_RETRY_MAX: Duration = Duration::from_secs(15 * 60);

/// When each repository's remote, keyed by git folder and remote, may be fetched next, and how
/// many fetches of it failed in a row.
static FETCHES: Mutex<Vec<(String, Instant, u32)>> = Mutex::new(Vec::new());

/// The settings for a git command nobody watches in the repository at `root`, so ssh fails
/// rather than ask for a passphrase or a new host's key, unless the user picked the ssh command
/// git runs.
pub(crate) fn unattended_ssh(root: &str) -> Vec<&'static str> {
    // One the user started and waits on asks them in the app instead.
    if super::askpass::may_ask(root)
        || crate::system::login_var("GIT_SSH_COMMAND").is_some()
        || crate::system::login_var("GIT_SSH").is_some()
        || git(root, &["config", "--get", "core.sshCommand"]).is_ok()
    {
        return Vec::new();
    }
    vec!["-c", "core.sshCommand=ssh -o BatchMode=yes"]
}

/// Fetches the remote of the repository at `root`, so comparisons read what the remote has.
///
/// A repository is fetched at most every 15 seconds, however many worktrees ask, and a failed
/// fetch waits longer each time before the next try. Callers that arrive while a fetch runs read
/// the refs as they are.
pub fn fetch_remote(root: &str) {
    let Some(remote) = push_remote(root) else {
        return;
    };
    let Ok(common) = git(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) else {
        return;
    };
    let key = format!("{}\n{remote}", common.trim());
    let now = Instant::now();
    let failures = {
        let mut fetches = FETCHES.lock().unwrap_or_else(PoisonError::into_inner);
        // Held until the fetch ends, so a caller meanwhile doesn't start another.
        let held = now + FETCH_RETRY_MAX;
        if let Some(entry) = fetches.iter_mut().find(|(k, ..)| *k == key) {
            if now < entry.1 {
                return;
            }
            entry.1 = held;
            entry.2
        } else {
            fetches.push((key.clone(), held, 0));
            0
        }
    };
    let mut args = unattended_ssh(root);
    // Pruned, so a branch the remote deleted, as a merged pull request's is, stops counting as
    // pushed.
    args.extend([
        "fetch",
        "--quiet",
        "--prune",
        "--no-tags",
        "--no-auto-gc",
        "--no-write-fetch-head",
        "--",
        &remote,
    ]);
    let result = git(root, &args);
    let (wait, failures) = match result {
        Ok(_) => {
            remember_default_branch(root, &remote);
            (FETCH_EVERY, 0)
        }
        Err(e) => {
            log::warn!(error:% = e; "could not fetch the project's remote: {{error}}");
            let wait = FETCH_RETRY
                .saturating_mul(2u32.saturating_pow(failures))
                .min(FETCH_RETRY_MAX);
            (wait, failures.saturating_add(1))
        }
    };
    let mut fetches = FETCHES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(entry) = fetches.iter_mut().find(|(k, ..)| *k == key) {
        entry.1 = Instant::now() + wait;
        entry.2 = failures;
    }
}

/// Asks `remote` which branch is its default, once, when the clone doesn't know: a repository
/// added with `git remote add` has no `<remote>/HEAD`, and guessing `main` or `master` gets a
/// default such as `develop` wrong.
fn remember_default_branch(root: &str, remote: &str) {
    let head = format!("refs/remotes/{remote}/HEAD");
    if git_succeeds(root, &["symbolic-ref", "-q", &head]) {
        return;
    }
    if let Err(e) = git(root, &["remote", "set-head", remote, "--auto"]) {
        log::warn!(error:% = e; "could not learn the remote's default branch: {{error}}");
    }
}

/// Whether everything `head` changed is on `base`, even under other commits, as a squash-merge
/// leaves it: merging `head` into `base` leaves `base`'s files as they are, or did at one of
/// `base`'s recent commits, since later commits may have changed the same lines again.
pub(crate) fn landed(root: &str, base: &str, head: &str) -> bool {
    merges_to_itself(root, base, head) || landed_earlier(root, base, head)
}

/// Whether merging `head` into `base` would leave `base`'s files as they are.
fn merges_to_itself(root: &str, base: &str, head: &str) -> bool {
    let merged = git(
        root,
        &["merge-tree", "--write-tree", "--no-messages", base, head],
    );
    let tree = git(root, &["rev-parse", &format!("{base}^{{tree}}")]);
    if let Err(e) = &merged
        && e.to_string().contains("unknown option")
    {
        // `merge-tree --write-tree` came in git 2.38; older git can't tell a squash-merge.
        static TOLD: std::sync::Once = std::sync::Once::new();
        TOLD.call_once(|| {
            log::warn!(error:% = e; "this git is too old to tell squash-merged branches (it needs 2.38 or newer): {{error}}");
        });
    }
    match (merged, tree) {
        (Ok(merged), Ok(tree)) => merged.lines().next().map(str::trim) == Some(tree.trim()),
        _ => false,
    }
}

/// How many of `base`'s commits since `head` branched off, among those touching its files,
/// [`landed_earlier`] tries, newest first.
const LANDED_SCAN: usize = 30;

/// What [`landed_earlier`] found, by `base` and `head` commit, since the sidebar asks often and
/// commits never change.
static LANDED_EARLIER: Mutex<Vec<(String, String, bool)>> = Mutex::new(Vec::new());

/// How many answers [`LANDED_EARLIER`] keeps.
const LANDED_KEPT: usize = 256;

/// Whether `head` had landed on one of `base`'s commits since it branched off that touch the
/// files it changed.
fn landed_earlier(root: &str, base: &str, head: &str) -> bool {
    let (Ok(base), Ok(head)) = (
        git(root, &["rev-parse", base]),
        git(root, &["rev-parse", head]),
    ) else {
        return false;
    };
    let (base, head) = (base.trim().to_owned(), head.trim().to_owned());
    let known = LANDED_EARLIER
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(b, h, _)| *b == base && *h == head)
        .map(|(_, _, found)| *found);
    if let Some(found) = known {
        return found;
    }
    let found = (|| {
        let fork = git(root, &["merge-base", &base, &head]).ok()?;
        let files = git(root, &["diff", "--name-only", fork.trim(), &head]).ok()?;
        let range = format!("{}..{base}", fork.trim());
        let max = LANDED_SCAN.to_string();
        let mut args = vec![
            "rev-list",
            "--first-parent",
            "--max-count",
            &max,
            &range,
            "--",
        ];
        args.extend(files.lines().filter(|f| !f.is_empty()));
        let candidates = git(root, &args).ok()?;
        Some(
            candidates
                .lines()
                .any(|c| merges_to_itself(root, c.trim(), &head)),
        )
    })()
    .unwrap_or(false);
    let mut cache = LANDED_EARLIER
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if cache.len() >= LANDED_KEPT {
        cache.clear();
    }
    cache.push((base, head, found));
    found
}

/// How long [`pr_merged`] trusts a "no" before asking GitHub again.
const MERGED_RECHECK: Duration = Duration::from_secs(10 * 60);

/// One answer of [`pr_merged`]: the repository, branch and commit, whether it was merged, and
/// when it asked.
type MergedAnswer = (String, String, String, bool, Instant);

/// What [`pr_merged`] found.
static PR_MERGED: Mutex<Vec<MergedAnswer>> = Mutex::new(Vec::new());

/// Whether a pull request from `branch` was merged into the default branch with its head at
/// commit `rev` in `root`, as `gh` tells, however it was merged. One merged into another branch,
/// such as the branch a stacked pull request was based on, doesn't count. A "yes" is kept; a
/// "no", or `gh` failing, is asked again after a while.
pub(crate) fn pr_merged(root: &str, branch: &str, rev: &str) -> bool {
    let Ok(head) = git(root, &["rev-parse", "--verify", "-q", rev]) else {
        return false;
    };
    let head = head.trim().to_owned();
    let found = |cache: &[MergedAnswer]| {
        cache
            .iter()
            .find(|(r, b, h, _, _)| r == root && b == branch && *h == head)
            .map(|(_, _, _, merged, at)| (*merged, *at))
    };
    let known = found(&PR_MERGED.lock().unwrap_or_else(PoisonError::into_inner));
    match known {
        Some((true, _)) => return true,
        Some((false, at)) if at.elapsed() < MERGED_RECHECK => return false,
        _ => {}
    }
    let Some(base) = default_branch(root, push_remote(root).as_deref()) else {
        return false;
    };
    let merged = gh(
        root,
        &[
            "pr",
            "list",
            "--head",
            branch,
            "--state",
            "merged",
            "--json",
            "headRefOid,baseRefName",
            "--limit",
            "20",
        ],
    )
    .ok()
    .and_then(|out| serde_json::from_str::<Vec<serde_json::Value>>(&out).ok())
    .is_some_and(|prs| {
        prs.iter().any(|pr| {
            pr["headRefOid"].as_str() == Some(&head) && pr["baseRefName"].as_str() == Some(&base)
        })
    });
    let mut cache = PR_MERGED.lock().unwrap_or_else(PoisonError::into_inner);
    cache.retain(|(r, b, h, _, _)| !(r == root && b == branch && *h == head));
    if cache.len() >= LANDED_KEPT {
        cache.clear();
    }
    cache.push((
        root.to_owned(),
        branch.to_owned(),
        head,
        merged,
        Instant::now(),
    ));
    merged
}

fn rev_count(root: &str, args: &[&str]) -> u32 {
    let mut full = vec!["rev-list", "--count"];
    full.extend(args);
    git(root, &full)
        .ok()
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0)
}

/// Where the branch stands against its remote, and what git stopped in the middle of.
///
/// Asks `gh` for an open pull request only on a pushed branch other than the default one, since
/// that call goes over the network.
#[must_use]
pub fn git_sync_state(root: &str) -> GitSyncState {
    let operation = operation(root);
    let remotes = push_remotes(root);
    let Some(branch) = git_branch(root).filter(|b| b != "HEAD") else {
        // A rebase runs with no branch checked out.
        return GitSyncState {
            operation,
            has_remote: !remotes.is_empty(),
            remotes,
            ..GitSyncState::default()
        };
    };
    let remote = remotes.first().cloned();
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
    let merged = !on_default
        && has_head(root)
        && base_ref(root).is_some_and(|base| landed(root, &base, "HEAD"));
    let pull_request = (has_upstream && !on_default && !merged)
        .then(|| open_pull_request(root))
        .flatten();
    GitSyncState {
        merged,
        has_changes: git(root, &["status", "--porcelain"]).is_ok_and(|s| !s.trim().is_empty()),
        has_remote: remote.is_some(),
        has_upstream,
        ahead,
        behind,
        pull_request,
        default_branch,
        branch: Some(branch),
        operation,
        remotes,
        prepared_message: prepared_message(root),
    }
}

/// Runs `gh <args>` in `root`, without prompts. A missing `gh` is said plainly, and one still
/// waiting on the network after git's limit for the network is stopped.
pub(crate) fn gh(root: &str, args: &[&str]) -> Result<String, Error> {
    let mut cmd = crate::system::host_command("gh");
    cmd.args(args)
        .current_dir(root)
        .env("GH_PROMPT_DISABLED", "1");
    let what = format!(
        "gh {}",
        args.iter().take(2).copied().collect::<Vec<_>>().join(" ")
    );
    let out = output_within(crate::system::ssh::prepare(cmd)?, NETWORK_LIMIT, &what).map_err(
        |e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::Message(
                "Pull requests need the GitHub CLI: install gh and run gh auth login".into(),
            ),
            _ => Error::Message(format!("Failed to run gh: {e}")),
        },
    )?;
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

/// The branch's open pull request as `gh pr view` gives it: its number, URL, checks, comments
/// and reviews.
///
/// # Errors
///
/// Returns an error when `gh` fails or the branch has no open pull request.
pub(crate) fn pull_request_view(root: &str) -> Result<serde_json::Value, Error> {
    let fields = "number,url,state,statusCheckRollup,comments,reviews";
    let out = gh(root, &["pr", "view", "--json", fields])?;
    let value: serde_json::Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    if value["state"] != "OPEN" {
        return Err("The branch has no open pull request".into());
    }
    Ok(value)
}

/// The checks of a pull request read by [`pull_request_view`].
pub(crate) fn pull_request_checks(
    pr: &serde_json::Value,
) -> impl Iterator<Item = &serde_json::Value> {
    pr["statusCheckRollup"].as_array().into_iter().flatten()
}

/// The branch's open pull request, if `gh` can tell, with where its checks stand and how many
/// comments and reviews it has.
fn open_pull_request(root: &str) -> Option<PullRequest> {
    let value = pull_request_view(root).ok()?;
    let states: Vec<CheckState> = pull_request_checks(&value).map(check_state).collect();
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
