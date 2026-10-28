//! Git reads and writes for a project folder. All functions are blocking.

use crate::Error;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::SystemTime;

use super::fs::resolve_inside;
use crate::types::FileChange;

/// Runs `git <args>` in `root` and returns its raw output.
fn git_raw(root: &str, args: &[&str]) -> Result<Output, Error> {
    crate::host_command("git")
        .args(args)
        .current_dir(root)
        // Reads such as `git status` would otherwise rewrite the index, which the project
        // watcher would report as a change.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::Git(format!("Failed to run git: {e}")))
}

/// Runs `git <args>` in `root` and returns stdout; a non-zero exit becomes an error carrying stderr.
pub(super) fn git(root: &str, args: &[&str]) -> Result<String, Error> {
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

fn is_untracked(root: &str, rel: &str) -> bool {
    !git_succeeds(root, &["ls-files", "--error-unmatch", "--", rel])
}

fn has_head(root: &str) -> bool {
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
    let out = git_raw(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
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
        let status = match code {
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
            staged: x != ' ' && x != '?',
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
