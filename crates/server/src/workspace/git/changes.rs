//! The working tree: its files, what changed in them, and keeping or reverting a change.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::SystemTime;

use crate::workspace::fs::resolve_inside;
use crate::workspace::FileChange;

use super::run::{git, git_failed, git_output, git_raw, git_succeeds, has_head, is_untracked};

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
            #[expect(
                clippy::naive_bytecount,
                reason = "counted once per changed file, not worth a dependency"
            )]
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
///
/// # Errors
///
/// Returns an error when `git ls-files` fails.
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
///
/// # Errors
///
/// Returns an error when `git` cannot be run for a reason other than being absent, or when `git
/// status` or `git diff` fails in a repository.
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
        let code = if x == ' ' { y } else { x };
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

/// The diff of `rel` against the last commit, or all of it when untracked.
///
/// # Errors
///
/// Returns an error when `git` fails to run or to diff the file.
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

/// Stages the changes to `rel`.
///
/// # Errors
///
/// Returns an error when `git add` fails for the path.
pub fn accept_file(root: &str, rel: &str) -> Result<(), Error> {
    git(root, &["add", "--", rel]).map(|_| ())
}

/// Throws away the changes to `rel`, deleting it when git does not track it.
///
/// # Errors
///
/// Returns an error when the path is outside the project, `git` cannot restore or remove it, or the
/// file cannot be deleted.
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
