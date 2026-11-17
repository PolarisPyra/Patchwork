//! Commits: staging and writing one, and reading the log.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashSet;

use crate::workspace::{GitCommit, GitRef};

use super::run::{git, git_branch, git_raw, git_succeeds, has_head};

/// Stages every change, untracked files included, for a commit of everything. Refused while a
/// file is still in conflict, since staging it would commit its conflict markers.
pub(crate) fn stage_all(root: &str) -> Result<(), Error> {
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
pub(crate) fn staged_summary(root: &str) -> Result<String, Error> {
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
///
/// # Errors
///
/// Returns an error when nothing is staged, `git` cannot run, or `git commit` fails, for example
/// when a hook refuses the commit.
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
///
/// # Errors
///
/// Returns an error when `git log`, `git remote`, or `git rev-list` fails.
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
