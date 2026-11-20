//! The working tree: its files, what changed in them, and keeping or reverting a change.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use crate::Error;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use crate::workspace::{DiffSide, FileChange};

use super::run::{git, git_failed, git_output, git_raw, git_succeeds, has_head, is_untracked};

/// Parses `git diff --numstat -z` into (path, additions, deletions). Renames
/// carry an extra NUL-separated origin path before the new path.
pub(super) fn parse_numstat(out: &str) -> Vec<(String, u32, u32)> {
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

/// Number of lines in an untracked file, as [`crate::system::fs::count_lines`] counts them. A
/// file is read again only once its size or modified time changes.
fn line_count(path: &Path) -> u32 {
    let stamp = std::fs::symlink_metadata(path).and_then(|m| Ok((m.len(), m.modified()?)));
    let Ok((len, modified)) = stamp else { return 0 };
    let mut cache = LINE_COUNTS.locked();
    if let Some(&(l, m, count)) = cache.get(path)
        && (l, m) == (len, modified)
    {
        return count;
    }
    let count = crate::system::fs::count_lines(path);
    cache.insert(path.to_path_buf(), (len, modified, count));
    count
}

/// Forgets the counts of files under `root` that are not in `untracked`, its untracked files now,
/// so files committed or deleted since are not kept.
fn forget_counts(root: &Path, untracked: &HashSet<PathBuf>) {
    LINE_COUNTS
        .locked()
        .retain(|path, _| !path.starts_with(root) || untracked.contains(path));
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
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
        ],
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
    // Git names each file from the repository's top, which the project may sit below; the rest of
    // Patchwork names it from the project.
    let prefix = if status.is_empty() {
        String::new()
    } else {
        let out = git(root, &["rev-parse", "--show-prefix"])?;
        out.trim_end_matches(['\r', '\n']).to_string()
    };

    let mut counts: HashMap<String, (u32, u32)> = HashMap::new();
    let tracked_changed = status
        .split('\0')
        .any(|entry| entry.len() >= 4 && !entry.starts_with("??"));
    if tracked_changed {
        // Staged and unstaged changes in one diff against the last commit, as the diff view shows
        // them: a renamed file by what changed in it. A repository with no commits yet has no
        // HEAD, so its two parts are added up.
        let numstats = match numstat(root, &["HEAD"]) {
            Ok(out) => vec![out],
            Err(_) => vec![numstat(root, &[])?, numstat(root, &["--cached"])?],
        };
        for (path, added, deleted) in numstats.iter().flat_map(|out| parse_numstat(out)) {
            let entry = counts.entry(path).or_default();
            entry.0 += added;
            entry.1 += deleted;
        }
    }
    // A file staged and changed again since is shown twice, each part with its own lines.
    let both_parts = status.split('\0').any(|entry| {
        let code = entry.as_bytes();
        entry.len() >= 4
            && !matches!(code[0], b' ' | b'?' | b'U')
            && !matches!(code[1], b' ' | b'U')
    });
    let part = |args: &[&str]| -> Result<HashMap<String, (u32, u32)>, Error> {
        if !both_parts {
            return Ok(HashMap::new());
        }
        Ok(parse_numstat(&numstat(root, args)?)
            .into_iter()
            .map(|(path, added, deleted)| (path, (added, deleted)))
            .collect())
    };
    let (staged_part, unstaged_part) = (part(&["--cached"])?, part(&[])?);

    // On an SSH host the untracked files are counted in one command rather than read here.
    let remote_counts = match crate::system::ssh::split(root) {
        Some(at) => {
            let untracked: Vec<String> = status
                .split('\0')
                .filter_map(|e| e.strip_prefix("?? "))
                .map(|p| p.strip_prefix(prefix.as_str()).unwrap_or(p).to_string())
                .collect();
            Some(crate::system::ssh::fs::line_counts(
                at.host, at.path, &untracked,
            )?)
        }
        None => None,
    };
    let name = |code: char| match code {
        '?' => "untracked",
        'A' => "added",
        'D' => "deleted",
        'R' => "renamed",
        _ => "modified",
    };
    let mut changes = Vec::new();
    let mut untracked = HashSet::new();
    let mut entries = status.split('\0');
    while let Some(entry) = entries.next() {
        // Each entry is "XY <path>"; rename/copy entries are followed by the origin path.
        if entry.len() < 4 {
            continue;
        }
        let (x, y) = (entry.as_bytes()[0] as char, entry.as_bytes()[1] as char);
        let path = &entry[3..];
        let path = path
            .strip_prefix(prefix.as_str())
            .unwrap_or(path)
            .to_string();
        let origin = (matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C'))
            .then(|| entries.next())
            .flatten()
            .map(|from| {
                from.strip_prefix(prefix.as_str())
                    .unwrap_or(from)
                    .to_string()
            });
        // A folder with a repository of its own inside is one entry ending in `/`: not a file to
        // stage, diff or delete.
        if path.ends_with('/') {
            continue;
        }
        // Both sides changed it in a merge, rebase, or cherry-pick, and git left it to resolve.
        let unmerged = x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D');
        let staged = !unmerged && x != ' ' && x != '?';
        let status = if unmerged {
            "conflicted"
        } else {
            name(if x == ' ' { y } else { x })
        };
        let (additions, deletions) = if x == '?' {
            if let Some(counts) = &remote_counts {
                (counts.get(&path).copied().unwrap_or(0), 0)
            } else {
                let file = Path::new(root).join(&path);
                let count = line_count(&file);
                untracked.insert(file);
                (count, 0)
            }
        } else {
            counts.get(&path).copied().unwrap_or((0, 0))
        };
        let worktree = (staged && y != ' ').then(|| name(y).to_string());
        let parts = worktree.as_ref().map(|_| {
            let lines = |part: &HashMap<String, (u32, u32)>| part.get(&path).copied();
            (
                lines(&staged_part).unwrap_or_default(),
                lines(&unstaged_part).unwrap_or_default(),
            )
        });
        changes.push(FileChange {
            renamed_from: origin.filter(|_| matches!(x, 'R') || matches!(y, 'R')),
            conflict: unmerged.then(|| conflict_kind(x, y).to_string()),
            parts,
            path,
            status: status.into(),
            additions,
            deletions,
            staged,
            worktree,
        });
    }
    if remote_counts.is_none() {
        forget_counts(Path::new(root), &untracked);
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(changes)
}

/// Runs `git diff --numstat` with `args`, renamed files counted by what changed in them.
fn numstat(root: &str, args: &[&str]) -> Result<String, Error> {
    let mut all = vec!["diff", "--numstat", "-z", "--relative", "-M"];
    all.extend_from_slice(args);
    git(root, &all)
}

/// How a file is in conflict, from the two letters `git status` gives it.
fn conflict_kind(x: char, y: char) -> &'static str {
    match (x, y) {
        ('D', 'D') => "both deleted",
        ('A', 'A') => "both added",
        ('A', 'U') => "added by us",
        ('U', 'A') => "added by them",
        ('D', 'U') => "deleted by us",
        ('U', 'D') => "deleted by them",
        _ => "both modified",
    }
}

/// The diff of `rel` against the last commit, or all of it when untracked. `side` narrows it to
/// the staged part, or to the part not staged. A renamed file is compared with the file it was.
///
/// # Errors
///
/// Returns an error when `git` fails to run or to diff the file.
pub fn git_file_diff(root: &str, rel: &str, side: DiffSide) -> Result<String, Error> {
    if is_untracked(root, rel)? {
        let out = git_raw(root, &["diff", "--no-index", "--", "/dev/null", rel])?;
        // `diff --no-index` exits 1 whenever the files differ, which is the normal case here.
        if !matches!(out.status.code(), Some(0 | 1)) {
            return Err(Error::Git(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let origin = renamed_from(root, rel)?;
    let diff = |how: &[&str]| {
        let mut args = vec!["diff", "-M"];
        args.extend_from_slice(how);
        args.push("--");
        args.extend(origin.as_deref());
        args.push(rel);
        git(root, &args)
    };
    match side {
        DiffSide::Staged => diff(&["--cached"]),
        DiffSide::Unstaged => diff(&[]),
        DiffSide::All if has_head(root) => diff(&["HEAD"]),
        // No commits yet, so there is no HEAD to diff against: show staged and unstaged parts.
        DiffSide::All => Ok(diff(&["--cached"])? + &diff(&[])?),
    }
}

/// Takes `rel` back out of the index, leaving the file as it is. A file staged as renamed has
/// its old name's deletion unstaged with it.
///
/// # Errors
///
/// Returns an error when `git` cannot unstage the path.
pub fn unstage_file(root: &str, rel: &str) -> Result<(), Error> {
    if !has_head(root) {
        return git(root, &["rm", "--cached", "-q", "--", rel]).map(|_| ());
    }
    let origin = renamed_from(root, rel)?;
    restore(root, &["--staged"], rel)?;
    match origin {
        Some(origin) => restore(root, &["--staged"], &origin),
        None => Ok(()),
    }
}

/// Takes everything under `root` back out of the index, leaving the files as they are.
///
/// # Errors
///
/// Returns an error when `git` cannot unstage the folder.
pub fn unstage_all(root: &str) -> Result<(), Error> {
    if has_head(root) {
        restore(root, &["--staged"], ".")
    } else {
        git(root, &["rm", "--cached", "-r", "-q", "--", "."]).map(|_| ())
    }
}

/// Whether the file `rel` still has the markers git writes around a conflict. A file that can't
/// be read, such as a deleted or binary one, has none.
#[must_use]
pub fn has_conflict_markers(root: &str, rel: &str) -> bool {
    if crate::system::ssh::split(root).is_some() {
        // `--check` exits non-zero when it finds any, and names them on its output.
        return git_raw(root, &["diff", "--check", "--", rel]).is_ok_and(|out| {
            String::from_utf8_lossy(&out.stdout).contains("leftover conflict marker")
        });
    }
    std::fs::read_to_string(Path::new(root).join(rel)).is_ok_and(|text| {
        let starts = |mark: &str| text.lines().any(|line| line.starts_with(mark));
        starts("<<<<<<< ") && starts(">>>>>>> ")
    })
}

/// Adds `rel` to the `.gitignore` at `root`. A file git already tracks stays tracked.
///
/// # Errors
///
/// Returns an error when the file cannot be read or written.
pub(crate) fn ignore(root: &str, rel: &str) -> Result<(), Error> {
    if crate::system::ssh::split(root).is_some() {
        return Err("Edit .gitignore on the host to ignore a file there".into());
    }
    let list = Path::new(root).join(".gitignore");
    let mut text = match std::fs::read_to_string(&list) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let line = format!("/{}", rel.trim_start_matches('/'));
    if text.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&line);
    text.push('\n');
    std::fs::write(&list, text).map_err(Error::from)
}

/// Stages the changes to `rel`.
///
/// # Errors
///
/// Returns an error when `git add` fails for the path.
pub fn accept_file(root: &str, rel: &str) -> Result<(), Error> {
    git(root, &["add", "--", rel]).map(|_| ())
}

/// Throws away the changes to `rel`, staged and not, deleting it when git does not track it. A
/// file staged as renamed gets its old name back.
///
/// # Errors
///
/// Returns an error when the path is outside the project, `git` cannot restore or remove it, or the
/// file cannot be deleted.
pub fn revert_file(root: &str, rel: &str) -> Result<(), Error> {
    if is_untracked(root, rel)? {
        return remove(root, rel);
    }
    if git_succeeds(root, &["cat-file", "-e", &format!("HEAD:./{rel}")]) {
        return restore(root, &["--source=HEAD", "--staged", "--worktree"], rel);
    }
    // Newly added file: drop it from the index and delete it from disk.
    let origin = renamed_from(root, rel)?;
    git(root, &["rm", "--cached", "-f", "-q", "--", rel])?;
    remove(root, rel)?;
    match origin {
        Some(origin) => restore(root, &["--source=HEAD", "--staged", "--worktree"], &origin),
        None => Ok(()),
    }
}

/// Throws away the changes to `rel` that are not staged, keeping what is, and deletes it when git
/// does not track it.
///
/// # Errors
///
/// Returns an error when the path is outside the project, or `git` cannot restore or remove it.
pub fn discard_unstaged(root: &str, rel: &str) -> Result<(), Error> {
    if is_untracked(root, rel)? {
        return remove(root, rel);
    }
    restore(root, &["--worktree"], rel)
}

/// Runs `git restore <how> -- rel`.
fn restore(root: &str, how: &[&str], rel: &str) -> Result<(), Error> {
    let mut args = vec!["restore"];
    args.extend_from_slice(how);
    args.extend_from_slice(&["--", rel]);
    git(root, &args).map(|_| ())
}

/// The path, from `root`, that the staged file `rel` was renamed from, if it was.
fn renamed_from(root: &str, rel: &str) -> Result<Option<String>, Error> {
    if !has_head(root) {
        return Ok(None);
    }
    let out = git(
        root,
        &[
            "diff",
            "--cached",
            "--name-status",
            "-z",
            "-M",
            "--relative",
            "HEAD",
        ],
    )?;
    let mut fields = out.split('\0');
    while let Some(status) = fields.next() {
        if status.starts_with(['R', 'C']) {
            let (Some(from), Some(to)) = (fields.next(), fields.next()) else {
                break;
            };
            if to == rel && status.starts_with('R') {
                return Ok(Some(from.to_string()));
            }
        } else {
            fields.next();
        }
    }
    Ok(None)
}

/// Removes the project file `rel`, refused outside the project. A link is removed itself, never
/// what it leads to, and a file already gone counts as removed.
///
/// On this computer the file goes to the Trash, where it can be got back, since git never had
/// it; it is deleted only when there is no Trash to move it to. On an SSH host it is deleted.
fn remove(root: &str, rel: &str) -> Result<(), Error> {
    if let Some(at) = crate::system::ssh::split(root) {
        return crate::system::ssh::fs::remove_inside(at.host, at.path, rel);
    }
    let path = crate::system::fs::entry_inside(Path::new(root), rel)?;
    if path.symlink_metadata().is_err() {
        return Ok(());
    }
    if let Err(e) = crate::system::fs::move_to_trash(&path) {
        log::warn!(path:% = path.display(), error:% = e; "could not move {{path}} to the Trash, deleting it: {{error}}");
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}
