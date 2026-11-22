//! One file's past: who last changed each of its lines, and the commits that changed it.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use crate::Error;
use crate::source_control::{BlameRange, FileRevision};

use super::run::{git, has_head};

/// What `git blame` gives as the hash of lines no commit has yet.
const UNCOMMITTED: char = '0';

/// What blame tells of one commit, given once however many ranges it has.
#[derive(Default, Clone)]
struct Blamed {
    author: String,
    time: i64,
    subject: String,
}

/// The commit that last changed each line of `rel` as it is on disk, as runs of lines in order.
///
/// Lines no commit has yet come with an empty hash. A file git does not track, or a folder with
/// no commits, has no runs.
///
/// # Errors
///
/// Returns an error when `git blame` fails, as it does for a binary file or a missing one.
pub fn git_blame(root: &str, rel: &str) -> Result<Vec<BlameRange>, Error> {
    if !has_head(root) || super::run::is_untracked(root, rel)? {
        return Ok(Vec::new());
    }
    let out = git(root, &["blame", "--incremental", "--", rel])?;
    let mut commits: HashMap<String, Blamed> = HashMap::new();
    let mut ranges: Vec<BlameRange> = Vec::new();
    // A run opens with `<hash> <line then> <line now> <count>`, and closes with its `filename`.
    let mut open: Option<(String, usize, usize)> = None;
    for line in out.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        let Some((hash, start, count)) = &open else {
            let mut numbers = value.split(' ').skip(1).map(str::parse::<usize>);
            if let (Some(Ok(start)), Some(Ok(count))) = (numbers.next(), numbers.next()) {
                commits.entry(key.to_string()).or_default();
                open = Some((key.to_string(), start.saturating_sub(1), count));
            }
            continue;
        };
        let Some(commit) = commits.get_mut(hash) else {
            continue;
        };
        match key {
            "author" => commit.author = value.to_string(),
            "author-time" => commit.time = value.parse().unwrap_or(0),
            "summary" => commit.subject = value.to_string(),
            "filename" => {
                ranges.push(BlameRange {
                    start: *start,
                    lines: *count,
                    hash: hash.clone(),
                    author: String::new(),
                    time: 0,
                    subject: String::new(),
                });
                open = None;
            }
            _ => {}
        }
    }
    ranges.sort_by_key(|r| r.start);
    let mut joined: Vec<BlameRange> = Vec::new();
    for mut range in ranges {
        let commit = commits.get(&range.hash).cloned().unwrap_or_default();
        if range.hash.chars().all(|c| c == UNCOMMITTED) {
            range.hash.clear();
        }
        match joined.last_mut() {
            Some(last) if last.hash == range.hash && last.start + last.lines == range.start => {
                last.lines += range.lines;
            }
            _ => joined.push(BlameRange {
                author: commit.author,
                time: commit.time,
                subject: commit.subject,
                ..range
            }),
        }
    }
    Ok(joined)
}

/// Up to `limit` commits that changed `rel`, after the first `skip`, newest first, followed
/// through its renames. Each comes with the name the file had in it.
///
/// # Errors
///
/// Returns an error when `git log` fails.
pub fn file_log(
    root: &str,
    rel: &str,
    skip: usize,
    limit: usize,
) -> Result<Vec<FileRevision>, Error> {
    if !has_head(root) {
        return Ok(Vec::new());
    }
    let skip = format!("--skip={skip}");
    let limit = format!("--max-count={limit}");
    let out = git(
        root,
        &[
            "-c",
            "i18n.logOutputEncoding=UTF-8",
            "log",
            "--no-show-signature",
            "--follow",
            "--name-only",
            "--format=%x1e%H%x1f%aN%x1f%at%x1f%s%x1f",
            &skip,
            &limit,
            "--",
            rel,
        ],
    )?;
    Ok(out
        .split('\x1e')
        .filter_map(|record| {
            let mut fields = record.split('\x1f');
            let hash = fields.next().filter(|h| !h.is_empty())?.to_string();
            let author = fields.next()?.to_string();
            let time = fields.next()?.parse().unwrap_or(0);
            let subject = fields.next()?.to_string();
            let path = fields.next().map(str::trim).filter(|p| !p.is_empty());
            Some(FileRevision {
                hash,
                author,
                time,
                subject,
                path: path.unwrap_or(rel).to_string(),
            })
        })
        .collect())
}

/// What the commit `hash` changed in the file at `rel`, as a patch against its first parent.
///
/// # Errors
///
/// Returns an error when `git show` fails, as for a hash the repository lacks.
pub fn commit_file_diff(root: &str, hash: &str, rel: &str) -> Result<String, Error> {
    git(
        root,
        &[
            "show",
            "--format=",
            "-M",
            "--diff-merges=first-parent",
            hash,
            "--",
            rel,
        ],
    )
}
