//! Commits: staging and writing one, and reading the log.

// Rust guideline compliant 2026-10-09

use patchwork_platform::Error;
use std::collections::{HashMap, HashSet};

use crate::model::{CommitDetail, CommitOptions, GitCommit, GitRef};

use super::branches::branch_base;
use super::run::{explained, git, git_branch, git_raw, git_succeeds, has_head};
use super::changes::parse_numstat;
use super::compare::name_status;

/// Stages every change under `root`, untracked files included, for a commit of everything there.
/// The rest of the repository stays as it is when `root` is one of its folders. Refused while a
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
    git(root, &["add", "-A", "--", "."]).map(|_| ())
}

/// Readies the index for a commit of `root`: what is staged there is committed as it is, and
/// everything is staged only when nothing is, or when `all` asks for it.
///
/// # Errors
///
/// Returns an error when a file is still in conflict, when `git` fails, or when `root` is a
/// folder of a larger repository that has files staged outside it, which the commit would take.
pub fn stage_for_commit(root: &str, all: bool) -> Result<(), Error> {
    if all || git_succeeds(root, &["diff", "--cached", "--quiet", "--", "."]) {
        stage_all(root)?;
    }
    if git(root, &["rev-parse", "--show-prefix"])?
        .trim()
        .is_empty()
    {
        return Ok(());
    }
    let staged = |within: &[&str]| -> Result<usize, Error> {
        let mut args = vec!["diff", "--cached", "--name-only", "-z"];
        args.extend_from_slice(within);
        Ok(git(root, &args)?
            .split('\0')
            .filter(|p| !p.is_empty())
            .count())
    };
    let outside = staged(&[])?.saturating_sub(staged(&["--", "."])?);
    if outside > 0 {
        return Err(Error::Message(format!(
            "{outside} staged {} outside this project's folder would go into the commit. Unstage \
             {} first.",
            if outside == 1 { "file" } else { "files" },
            if outside == 1 { "it" } else { "them" },
        )));
    }
    Ok(())
}

/// The first `max` characters of `text`, marked when cut.
#[must_use]
pub fn truncated(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}\n[truncated]", &text[..end]),
        None => text.to_string(),
    }
}

/// What a commit message is written from: the branch, the staged files and their patch, cut to
/// fit one command line on every system.
///
/// # Errors
///
/// Returns an error when nothing is staged or git diff fails.
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

/// Commits what is staged with `message`, and returns the new commit's hash.
///
/// The repository's hooks run unless `options` skips them, and a hook that refuses the commit is
/// quoted, from both of its outputs. Amending with a blank `message` keeps the last commit's.
///
/// # Errors
///
/// Returns an error when nothing is staged, `git` cannot run, or `git commit` fails, for example
/// when a hook refuses the commit or git has no name and email to sign it with.
pub fn commit(root: &str, message: &str, options: CommitOptions) -> Result<String, Error> {
    // A merge whose result is the branch as it was still has a merge commit to write.
    if !options.amend
        && !options.allow_empty
        && git_succeeds(root, &["diff", "--cached", "--quiet"])
        && !git_succeeds(root, &["rev-parse", "-q", "--verify", "MERGE_HEAD"])
    {
        return Err(Error::Message("Nothing to commit".into()));
    }
    let mut args = vec!["commit"];
    for (on, flag) in [
        (options.amend, "--amend"),
        (options.signoff, "--signoff"),
        (options.no_verify, "--no-verify"),
        (options.allow_empty, "--allow-empty"),
    ] {
        if on {
            args.push(flag);
        }
    }
    if options.amend && message.trim().is_empty() {
        args.push("--no-edit");
    } else {
        args.extend(["-m", message]);
    }
    let out = git_raw(root, &args)?;
    if !out.status.success() {
        let said = [&out.stderr, &out.stdout]
            .map(|o| String::from_utf8_lossy(o).trim().to_string())
            .into_iter()
            .filter(|o| !o.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(explained(Error::Git(format!("git commit: {said}"))));
    }
    Ok(git(root, &["rev-parse", "HEAD"])?.trim().to_string())
}

/// A commit's message body, its author's email, and the files it changed against its first
/// parent.
///
/// # Errors
///
/// Returns an error when `git show` fails, as for a hash the repository lacks.
pub fn commit_detail(root: &str, hash: &str) -> Result<CommitDetail, Error> {
    let said = git(root, &["show", "-s", "--format=%aE%x1f%b", hash, "--"])?;
    let (email, body) = said.split_once('\x1f').unwrap_or_default();
    let show = |what: &str| {
        git(
            root,
            &[
                "show",
                "--format=",
                "-z",
                "-M",
                "--diff-merges=first-parent",
                what,
                hash,
                "--",
            ],
        )
    };
    let counts: HashMap<String, (u32, u32)> = parse_numstat(&show("--numstat")?)
        .into_iter()
        .map(|(path, added, deleted)| (path, (added, deleted)))
        .collect();
    let files = name_status(&show("--name-status")?, &counts);
    Ok(CommitDetail {
        body: body.trim().to_string(),
        email: email.to_string(),
        files,
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

/// Up to `limit` commits after the first `skip`, newest first in graph order.
///
/// They are those of the checked-out branch, its upstream, and the branch it was made from, else
/// the remote's default one. A folder without commits has none.
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
        "-c",
        "i18n.logOutputEncoding=UTF-8",
        "log",
        "--no-show-signature",
        "--topo-order",
        "--decorate=full",
        "--format=%H%x1f%P%x1f%aN%x1f%at%x1f%D%x1f%s%x1e",
        &skip,
        &limit,
        "HEAD",
    ];
    if git_succeeds(root, &["rev-parse", "--verify", "-q", "@{u}"]) {
        args.push("@{u}");
    }
    // The branch this one goes back into, so what it has that the branch lacks shows too.
    let base = branch_base(root);
    if let Some(base) = &base {
        args.push(base);
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
                chat_id: None,
                replies: Vec::new(),
            })
        })
        .collect())
}

/// The files under `root` changed by commits since `since` (seconds since the epoch).
///
/// A file an agent changed and committed in one turn is in neither `git status` before nor after
/// it; this finds it by committer date.
#[must_use]
pub fn committed_since(root: &str, since: u64) -> Vec<String> {
    let since = format!("--since=@{since}");
    let args = [
        "log",
        &since,
        "--no-merges",
        "--format=",
        "--name-only",
        "-z",
        "--relative",
        "--",
        ".",
    ];
    // A folder with no commits yet, or no repository, has none.
    if !has_head(root) {
        return Vec::new();
    }
    match git(root, &args) {
        Ok(out) => {
            let mut seen = HashSet::new();
            out.split('\0')
                .filter(|p| !p.is_empty() && seen.insert(*p))
                .map(String::from)
                .collect()
        }
        Err(e) => {
            log::warn!(error:% = e; "could not list the files a turn's commits changed: {{error}}");
            Vec::new()
        }
    }
}
