//! What the project watcher never watches or reports.

use std::path::Path;

#[cfg(target_os = "linux")]
use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// Folders never watched, even where no `.gitignore` names them.
pub(super) const SKIPPED_DIRS: [&str; 3] = [".git", "node_modules", "target"];

/// Decides which event paths are reported.
pub(super) struct Filter {
    /// The root `.gitignore` and `.git/info/exclude`. Nested ignore files are left out: git
    /// leaves the paths they name out of its status anyway, so missing one costs a refresh, not
    /// a wrong list.
    ignored: Gitignore,
}

impl Filter {
    pub(super) fn new(root: &Path) -> Self {
        let mut builder = GitignoreBuilder::new(root);
        builder.add(root.join(".gitignore"));
        builder.add(root.join(".git/info/exclude"));
        let ignored = builder.build().unwrap_or_else(|_| Gitignore::empty());
        Self { ignored }
    }

    /// Whether a change to `path`, which is `rel` under the root, is reported. In `.git` only
    /// its top level counts, without git's index lock and watchman's cookie files.
    pub(super) fn keeps(&self, path: &Path, rel: &Path) -> bool {
        let mut parts = rel.iter();
        let Some(first) = parts.next() else { return false };
        if first == ".git" {
            let Some(name) = parts.next() else { return false };
            return parts.next().is_none() && name != "index.lock" && !is_cookie(name);
        }
        if is_cookie(rel.file_name().unwrap_or_default()) {
            return false;
        }
        let skipped = rel.iter().any(|part| SKIPPED_DIRS.iter().any(|dir| part == *dir));
        !skipped && !self.ignored.matched_path_or_any_parents(path, path.is_dir()).is_ignore()
    }
}

/// Watchman creates a cookie file in the watched tree whenever it is queried.
fn is_cookie(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with(".watchman-cookie-")
}

/// Walks `dir` the way git sees it, hidden folders included, every ignore file applied.
#[cfg(target_os = "linux")]
pub(super) fn walker(dir: &Path) -> WalkBuilder {
    let mut walk = WalkBuilder::new(dir);
    walk.hidden(false)
        .filter_entry(|entry| !SKIPPED_DIRS.iter().any(|skip| entry.file_name() == *skip));
    walk
}
