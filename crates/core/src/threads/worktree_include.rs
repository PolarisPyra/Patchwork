//! The ignored files a new worktree starts with: those the project's `.worktreeinclude` names.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::path::Path;

use super::worktree::git;

/// Copies into a new worktree at `to` the files git ignores in the repository at `top` that its
/// `.worktreeinclude` names, in `.gitignore`'s form, such as `.env`: a worktree starts with only
/// what git tracks. Nothing the worktree already has is replaced, a link is copied as a link,
/// and a file that can't be copied is logged and left out.
pub(super) fn copy_included(top: &str, to: &str) {
    let list = Path::new(top).join(".worktreeinclude");
    if !list.is_file() {
        return;
    }
    let from = format!("--exclude-from={}", list.display());
    let ignored = |more: &[&str]| {
        let mut args = vec!["ls-files", "-z", "--others", "--ignored"];
        args.extend_from_slice(more);
        git(top, &args)
    };
    let lists = ignored(&["--exclude-standard"])
        .and_then(|all| Ok((all, ignored(&[&from])?, git(to, &["ls-files", "-z"])?)));
    let (ignored, named, tracked) = match lists {
        Ok(lists) => lists,
        Err(e) => {
            log::warn!(error:% = e; "could not read the project's .worktreeinclude: {{error}}");
            return;
        }
    };
    // Only saves copying file by file, so the copy goes ahead without it.
    let folders = git(
        top,
        &[
            "ls-files",
            "-z",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "--no-empty-directory",
        ],
    )
    .unwrap_or_default();
    for rel in include_paths(&ignored, &named, &folders, &tracked) {
        let target = Path::new(to).join(&rel);
        if target.symlink_metadata().is_ok() {
            continue;
        }
        let copied = target
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| copy_entry(&Path::new(top).join(&rel), &target));
        if let Err(e) = copied {
            log::warn!(file:% = rel, error:% = e; "could not copy {{file}} into a task's worktree: {{error}}");
        }
    }
}

/// The paths to copy, from git's NUL-separated lists: `ignored`, every file the repository
/// ignores; `named`, the files `.worktreeinclude` matches; `folders`, the ignored files with a
/// folder git ignores whole as one `name/` entry; and `tracked`, the files the new worktree
/// checked out.
///
/// A file is copied when git ignores it, `.worktreeinclude` names it, and it would not replace or
/// sit inside a tracked path. A folder ignored whole whose every file is copied is one entry,
/// copied whole, in place of its files.
fn include_paths(ignored: &str, named: &str, folders: &str, tracked: &str) -> Vec<String> {
    let named: HashSet<&str> = entries(named).collect();
    let whole: HashSet<&str> = entries(folders).filter(|e| e.ends_with('/')).collect();
    let tracked: HashSet<&str> = entries(tracked).collect();
    let tracked_folders: HashSet<&str> = tracked.iter().flat_map(|file| folders_of(file)).collect();
    // A tracked file where the copy would go, a tracked folder of its name, or a tracked file
    // where one of its folders would be.
    let collides = |file: &str| {
        tracked.contains(file)
            || tracked_folders.contains(format!("{file}/").as_str())
            || folders_of(file).any(|folder| tracked.contains(&folder[..folder.len() - 1]))
    };

    let mut files = Vec::new();
    let mut partial = HashSet::new();
    for file in entries(ignored) {
        if named.contains(file) && !collides(file) {
            files.push(file);
        } else if let Some(folder) = within(file, &whole) {
            partial.insert(folder);
        }
    }
    if files.is_empty() {
        return Vec::new();
    }
    let copied_whole =
        |file: &str| within(file, &whole).is_some_and(|folder| !partial.contains(folder));
    let mut paths: Vec<String> = whole
        .iter()
        .filter(|folder| !partial.contains(*folder))
        .map(|folder| folder[..folder.len() - 1].to_string())
        .collect();
    paths.sort_unstable();
    paths.extend(
        files
            .iter()
            .filter(|file| !copied_whole(file))
            .map(|file| (*file).to_string()),
    );
    paths
}

/// The folder among `whole`, those git ignores whole, that `file` is in.
fn within<'a>(file: &'a str, whole: &HashSet<&str>) -> Option<&'a str> {
    folders_of(file).find(|folder| whole.contains(folder))
}

/// The entries of one of git's NUL-separated lists.
fn entries(list: &str) -> impl Iterator<Item = &str> {
    list.split('\0').filter(|entry| !entry.is_empty())
}

/// The folders `path` is in, outermost first, each ending in `/` as git writes a folder.
fn folders_of(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/').map(move |(at, _)| &path[..=at])
}

/// Copies the file, link or folder at `from` to `to`, which doesn't exist. A link is made again
/// as a link rather than followed, and what is neither, such as a socket, is left out.
fn copy_entry(from: &Path, to: &Path) -> std::io::Result<()> {
    let kind = from.symlink_metadata()?.file_type();
    if kind.is_symlink() {
        #[cfg(unix)]
        return std::os::unix::fs::symlink(std::fs::read_link(from)?, to);
        #[cfg(not(unix))]
        return std::fs::copy(from, to).map(drop);
    }
    if kind.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_entry(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if kind.is_file() {
        std::fs::copy(from, to)?;
    }
    Ok(())
}
