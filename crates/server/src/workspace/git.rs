//! Git reads and writes for a project folder. All functions are blocking.

use crate::Error;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::SystemTime;

use super::fs::resolve_inside;
use crate::workspace::{FileChange, GitCommit, GitRef, GitSyncState, PullRequest};

/// Runs `git <args>` in `root` and returns its raw output.
fn git_raw(root: &str, args: &[&str]) -> Result<Output, Error> {
    git_output(root, args).map_err(git_failed)
}

/// `git_raw` with the error from starting git as it came, so a caller can tell git is missing.
fn git_output(root: &str, args: &[&str]) -> std::io::Result<Output> {
    crate::system::host_command("git")
        .args(args)
        .current_dir(root)
        // Reads such as `git status` would otherwise rewrite the index, which the project
        // watcher would report as a change.
        .env("GIT_OPTIONAL_LOCKS", "0")
        // There is no terminal to type a password in, so a push fails instead of waiting.
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
}

fn git_failed(e: std::io::Error) -> Error {
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

fn git_succeeds(root: &str, args: &[&str]) -> bool {
    git_raw(root, args).is_ok_and(|o| o.status.success())
}

pub(super) fn is_untracked(root: &str, rel: &str) -> bool {
    !git_succeeds(root, &["ls-files", "--error-unmatch", "--", rel])
}

pub(super) fn has_head(root: &str) -> bool {
    git_succeeds(root, &["rev-parse", "--verify", "-q", "HEAD"])
}

pub fn git_branch(root: &str) -> Option<String> {
    git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Parses `git diff --numstat -z` into (path, additions, deletions). Renames
/// carry an extra NUL-separated origin path before the new path.
fn parse_numstat(out: &str) -> Vec<(String, u32, u32)> {
    let mut entries = Vec::new();
    let mut tokens = out.split('\0');
    while let Some(token) = tokens.next() {
        let mut parts = token.splitn(3, '\t');
        let (Some(added), Some(deleted), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let path = if path.is_empty() {
            tokens.next(); // origin path of a rename
            match tokens.next() {
                Some(new) => new.to_string(),
                None => break,
            }
        } else {
            path.to_string()
        };
        // Binary files report "-" for both counts.
        entries.push((
            path,
            added.parse().unwrap_or(0),
            deleted.parse().unwrap_or(0),
        ));
    }
    entries
}

/// A file's size and modified time, and the lines it had then.
type Counted = (u64, SystemTime, u32);

/// Line counts of untracked files, by path.
static LINE_COUNTS: LazyLock<Mutex<HashMap<PathBuf, Counted>>> = LazyLock::new(Default::default);

/// Number of lines in an untracked file; 0 for binary or unreadable files. A file is read again
/// only once its size or modified time changes.
fn line_count(path: &Path) -> u32 {
    let stamp = std::fs::metadata(path).and_then(|m| Ok((m.len(), m.modified()?)));
    let Ok((len, modified)) = stamp else { return 0 };
    let mut cache = LINE_COUNTS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(&(l, m, count)) = cache.get(path)
        && (l, m) == (len, modified)
    {
        return count;
    }
    let count = count_lines(path);
    cache.insert(path.to_path_buf(), (len, modified, count));
    count
}

fn count_lines(path: &Path) -> u32 {
    match std::fs::read(path) {
        Ok(bytes) if !bytes.contains(&0) => {
            let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
            let partial_last = bytes.last().is_some_and(|&b| b != b'\n');
            (newlines + usize::from(partial_last)) as u32
        }
        _ => 0,
    }
}

/// Most files a mention menu offers; a large repo is cut off here.
const MAX_LISTED_FILES: usize = 5000;

/// Files git knows about: tracked files and untracked ones that are not ignored. A folder
/// that is not a git repository has none.
pub fn list_files(root: &str) -> Result<Vec<String>, Error> {
    if !git_succeeds(root, &["rev-parse", "--is-inside-work-tree"]) {
        return Ok(vec![]);
    }
    let out = git(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    Ok(out
        .split('\0')
        .filter(|path| !path.is_empty())
        .take(MAX_LISTED_FILES)
        .map(String::from)
        .collect())
}

/// The project's uncommitted changes with their line counts: `git status`, plus one
/// `git diff HEAD` for the counts when a tracked file changed.
pub fn git_changes(root: &str) -> Result<Vec<FileChange>, Error> {
    let out = match git_output(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    ) {
        Ok(out) => out,
        // Without git installed, a folder has no changes, as one that is not a repository.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(git_failed(e)),
    };
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        // A folder that is not a git repository has no changes.
        if err.contains("not a git repository") {
            return Ok(vec![]);
        }
        return Err(Error::Git(format!("git status: {}", err.trim())));
    }
    let status = String::from_utf8_lossy(&out.stdout).into_owned();

    let mut counts: HashMap<String, (u32, u32)> = HashMap::new();
    let tracked_changed = status
        .split('\0')
        .any(|entry| entry.len() >= 4 && !entry.starts_with("??"));
    if tracked_changed {
        // Staged and unstaged changes in one diff against the last commit, as the diff view shows
        // them. A repository with no commits yet has no HEAD, so its two parts are added up.
        let numstats = match git(root, &["diff", "HEAD", "--numstat", "-z"]) {
            Ok(out) => vec![out],
            Err(_) => vec![
                git(root, &["diff", "--numstat", "-z"])?,
                git(root, &["diff", "--cached", "--numstat", "-z"])?,
            ],
        };
        for (path, added, deleted) in numstats.iter().flat_map(|out| parse_numstat(out)) {
            let entry = counts.entry(path).or_default();
            entry.0 += added;
            entry.1 += deleted;
        }
    }

    let mut changes = Vec::new();
    let mut entries = status.split('\0');
    while let Some(entry) = entries.next() {
        // Each entry is "XY <path>"; rename/copy entries are followed by the origin path.
        if entry.len() < 4 {
            continue;
        }
        let (x, y) = (entry.as_bytes()[0] as char, entry.as_bytes()[1] as char);
        let path = entry[3..].to_string();
        if x == 'R' || x == 'C' {
            entries.next();
        }
        let code = if x != ' ' { x } else { y };
        // Both sides changed it in a merge, rebase, or cherry-pick, and git left it to resolve.
        let unmerged = x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D');
        let status = match code {
            _ if unmerged => "conflicted",
            '?' => "untracked",
            'A' => "added",
            'D' => "deleted",
            'R' => "renamed",
            _ => "modified",
        };
        let (additions, deletions) = if x == '?' {
            (line_count(&Path::new(root).join(&path)), 0)
        } else {
            counts.get(&path).copied().unwrap_or((0, 0))
        };
        changes.push(FileChange {
            path,
            status: status.into(),
            additions,
            deletions,
            staged: !unmerged && x != ' ' && x != '?',
        });
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(changes)
}

pub fn git_file_diff(root: &str, rel: &str) -> Result<String, Error> {
    if is_untracked(root, rel) {
        let out = git_raw(root, &["diff", "--no-index", "--", "/dev/null", rel])?;
        // `diff --no-index` exits 1 whenever the files differ, which is the normal case here.
        if !matches!(out.status.code(), Some(0 | 1)) {
            return Err(Error::Git(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    if has_head(root) {
        return git(root, &["diff", "HEAD", "--", rel]);
    }
    // No commits yet, so there is no HEAD to diff against: show staged and unstaged parts.
    Ok(git(root, &["diff", "--cached", "--", rel])? + &git(root, &["diff", "--", rel])?)
}

pub fn accept_file(root: &str, rel: &str) -> Result<(), Error> {
    git(root, &["add", "--", rel]).map(|_| ())
}

pub fn revert_file(root: &str, rel: &str) -> Result<(), Error> {
    if is_untracked(root, rel) {
        return std::fs::remove_file(resolve_inside(root, rel)?).map_err(Error::from);
    }
    if git_succeeds(root, &["cat-file", "-e", &format!("HEAD:{rel}")]) {
        git(
            root,
            &[
                "restore",
                "--source=HEAD",
                "--staged",
                "--worktree",
                "--",
                rel,
            ],
        )?;
    } else {
        // Newly added file: drop it from the index and delete it from disk.
        git(root, &["rm", "--cached", "-f", "-q", "--", rel])?;
        if let Ok(path) = resolve_inside(root, rel) {
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

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
        .map(|r| r.to_string())
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
    let out = crate::system::host_command("gh")
        .args(args)
        .current_dir(root)
        .env("GH_PROMPT_DISABLED", "1")
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

/// Stages every change, untracked files included, for a commit of everything. Refused while a
/// file is still in conflict, since staging it would commit its conflict markers.
pub fn stage_all(root: &str) -> Result<(), Error> {
    if !git(root, &["diff", "--name-only", "--diff-filter=U"])?
        .trim()
        .is_empty()
    {
        return Err(Error::Message(
            "Resolve the merge conflicts before committing".into(),
        ));
    }
    git(root, &["add", "-A"]).map(|_| ())
}

/// The first `max` characters of `text`, marked when cut.
pub(crate) fn truncated(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}\n[truncated]", &text[..end]),
        None => text.to_string(),
    }
}

/// What a commit message is written from: the branch, the staged files and their patch, cut to
/// fit one command line on every system.
pub fn staged_summary(root: &str) -> Result<String, Error> {
    let files = git(root, &["diff", "--cached", "--name-status"])?;
    if files.trim().is_empty() {
        return Err(Error::Message("Nothing to commit".into()));
    }
    let patch = git(
        root,
        &[
            "diff",
            "--cached",
            "--no-color",
            "--no-ext-diff",
            "--minimal",
        ],
    )?;
    let branch = git_branch(root).unwrap_or_else(|| "(detached)".into());
    Ok(format!(
        "Branch: {branch}\n\nStaged files:\n{}\n\nStaged patch:\n{}",
        truncated(&files, 4_000),
        truncated(&patch, 16_000)
    ))
}

/// Commits what is staged with `message`, running the repository's hooks, and returns the new
/// commit's hash. A hook that refuses it is quoted, from both of its outputs.
pub fn commit(root: &str, message: &str) -> Result<String, Error> {
    if git_succeeds(root, &["diff", "--cached", "--quiet"]) {
        return Err(Error::Message("Nothing to commit".into()));
    }
    let out = git_raw(root, &["commit", "-m", message])?;
    if !out.status.success() {
        let said = [&out.stderr, &out.stdout]
            .map(|o| String::from_utf8_lossy(o).trim().to_string())
            .into_iter()
            .filter(|o| !o.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(Error::Git(format!("git commit: {said}")));
    }
    Ok(git(root, &["rev-parse", "HEAD"])?.trim().to_string())
}

/// Pushes the branch. Its first push sets the upstream on `origin`, or the first remote.
/// Returns where it went, such as `origin/main`.
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
pub fn branch_summary(root: &str) -> Result<(String, String), Error> {
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
pub fn create_pull_request(
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

/// Reads `%D` with full ref names, such as `HEAD -> refs/heads/main, tag: refs/tags/v1`.
fn parse_refs(decoration: &str) -> Vec<GitRef> {
    decoration
        .split(", ")
        .filter_map(|part| {
            let (head, name) = match part.strip_prefix("HEAD -> ") {
                Some(name) => (true, name),
                None => (false, part.strip_prefix("tag: ").unwrap_or(part)),
            };
            let (kind, short) = if let Some(n) = name.strip_prefix("refs/heads/") {
                (if head { "head" } else { "branch" }, n)
            } else if let Some(n) = name.strip_prefix("refs/remotes/") {
                ("remote", n)
            } else {
                ("tag", name.strip_prefix("refs/tags/")?)
            };
            // `origin/HEAD` only repeats the remote's default branch.
            (!short.ends_with("/HEAD")).then(|| GitRef {
                name: short.to_string(),
                kind: kind.to_string(),
            })
        })
        .collect()
}

/// Up to `limit` commits after the first `skip`, newest first in graph order: the checked-out
/// branch and its upstream. A folder without commits has none.
pub fn git_log(root: &str, skip: usize, limit: usize) -> Result<Vec<GitCommit>, Error> {
    if !has_head(root) {
        return Ok(vec![]);
    }
    let skip = format!("--skip={skip}");
    let limit = format!("--max-count={limit}");
    let mut args = vec![
        "log",
        "--topo-order",
        "--decorate=full",
        "--format=%H%x1f%P%x1f%an%x1f%at%x1f%D%x1f%s%x1e",
        &skip,
        &limit,
        "HEAD",
    ];
    if git_succeeds(root, &["rev-parse", "--verify", "-q", "@{u}"]) {
        args.push("@{u}");
    }
    args.push("--");
    let out = git(root, &args)?;
    // Commits no remote branch has are not pushed yet; with no remote there is nowhere to push.
    let unpushed: HashSet<String> = if git(root, &["remote"])?.trim().is_empty() {
        HashSet::new()
    } else {
        git(root, &["rev-list", "HEAD", "--not", "--remotes"])?
            .lines()
            .map(String::from)
            .collect()
    };
    Ok(out
        .split('\x1e')
        .filter_map(|record| {
            let mut fields = record.trim_start_matches('\n').split('\x1f');
            let hash = fields.next().filter(|h| !h.is_empty())?.to_string();
            Some(GitCommit {
                pushed: !unpushed.contains(&hash),
                hash,
                parents: fields
                    .next()?
                    .split_whitespace()
                    .map(String::from)
                    .collect(),
                author: fields.next()?.to_string(),
                time: fields.next()?.parse().unwrap_or(0),
                refs: parse_refs(fields.next()?),
                subject: fields.next()?.to_string(),
                thread_id: None,
                replies: Vec::new(),
            })
        })
        .collect())
}
