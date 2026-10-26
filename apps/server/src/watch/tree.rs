//! FSEvents and ReadDirectoryChangesW watch a whole tree for the price of one folder, so the
//! root gets one recursive watch and ignored paths are dropped as their events arrive.

use std::path::{Path, PathBuf};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// Watches the whole project, `.git` included, in one recursive watch.
pub(super) fn watch_project(watcher: &mut RecommendedWatcher, root: &Path) {
    if let Err(e) = watcher.watch(root, RecursiveMode::Recursive) {
        log::warn!("failed to watch {}: {e}", root.display());
    }
}

/// A new folder is inside the recursive watch already, and its files report their own events.
pub(super) fn dir_created(_watcher: &mut RecommendedWatcher, _dir: &Path) -> Vec<PathBuf> {
    Vec::new()
}
