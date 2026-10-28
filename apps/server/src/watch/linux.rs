//! inotify watches one folder at a time, so each folder git keeps gets its own watch, and
//! ignored folders such as build output cost nothing.

use std::path::{Path, PathBuf};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use super::filter::walker;

/// Watches every folder of the working tree that git keeps, and the top level of `.git`.
pub(super) fn watch_project(watcher: &mut RecommendedWatcher, root: &Path) {
    watch_tree(watcher, root);
    // In a linked worktree `.git` is a file, and only the working tree is watched.
    let git_dir = root.join(".git");
    if git_dir.is_dir() {
        let _ = watcher.watch(&git_dir, RecursiveMode::NonRecursive);
    }
}

/// Watches a folder created after the start, unless git ignores it, and returns the files
/// already in it, which were written before its watch was in place. Listing its parent applies
/// every ignore file above it.
pub(super) fn dir_created(watcher: &mut RecommendedWatcher, dir: &Path) -> Vec<PathBuf> {
    let Some(parent) = dir.parent() else {
        return Vec::new();
    };
    let kept = walker(parent)
        .max_depth(Some(1))
        .build()
        .flatten()
        .any(|entry| entry.path() == dir);
    if kept {
        watch_tree(watcher, dir)
    } else {
        Vec::new()
    }
}

/// Adds a watch on `dir` and each folder under it that git keeps; returns the files found.
fn watch_tree(watcher: &mut RecommendedWatcher, dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in walker(dir).build().flatten() {
        if entry.file_type().is_some_and(|t| t.is_dir()) {
            let _ = watcher.watch(entry.path(), RecursiveMode::NonRecursive);
        } else {
            files.push(entry.into_path());
        }
    }
    files
}
