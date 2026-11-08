//! Watches a project the way an editor's git view does: the working tree minus what git ignores,
//! plus the top level of `.git` for commits, staging and checkouts, and its branches and tags for
//! pushes and fetches. A burst of events is reported once, after it settles, with when each path
//! changed.
//!
//! How folders are watched depends on the platform: inotify watches one folder at a time, so
//! Linux adds a watch per folder git keeps; FSEvents and ReadDirectoryChangesW watch a whole tree
//! for the price of one, so other platforms watch the root and drop ignored paths as they arrive.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::{Event, EventKind, RecommendedWatcher};

#[cfg(target_os = "linux")]
use linux as backend;
#[cfg(not(target_os = "linux"))]
use tree as backend;

/// Quiet time that ends a burst of events.
const SETTLE: Duration = Duration::from_millis(250);
/// Longest a steady stream of events holds back a report.
const MAX_WAIT: Duration = Duration::from_secs(1);

/// A project-relative path, with `/` between its parts as git writes them, and when it last
/// changed in the burst.
#[derive(Debug, Clone)]
pub struct Change {
    pub path: String,
    pub at: Instant,
}

/// Watches until dropped.
pub struct ProjectWatcher {
    _watcher: Arc<Mutex<RecommendedWatcher>>,
}

impl ProjectWatcher {
    /// Starts watching `root`. Folders are added on a thread of the watcher's own, so this
    /// returns at once. `on_change` runs on that thread with what a burst changed; events that
    /// arrive meanwhile wait for the next call.
    pub fn start(
        root: &Path,
        on_change: impl FnMut(Vec<Change>) + Send + 'static,
    ) -> Result<Self, String> {
        // Events name paths under the root as the system resolves it, such as `/private/var`
        // for `/var` on macOS.
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let (tx, rx) = channel();
        let watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send((Instant::now(), res));
        })
        .map_err(|e| e.to_string())?;
        let watcher = Arc::new(Mutex::new(watcher));
        let weak = Arc::downgrade(&watcher);
        std::thread::Builder::new()
            .name("project-watch".into())
            .spawn(move || run(root, weak, rx, on_change))
            .map_err(|e| e.to_string())?;
        Ok(Self { _watcher: watcher })
    }
}

type Received = (Instant, notify::Result<Event>);

/// The watcher's thread. It ends once the watcher is dropped, which closes the channel.
fn run(
    root: PathBuf,
    watcher: Weak<Mutex<RecommendedWatcher>>,
    rx: Receiver<Received>,
    mut on_change: impl FnMut(Vec<Change>),
) {
    let Some(shared) = watcher.upgrade() else {
        return;
    };
    backend::watch_project(&mut lock(&shared), &root);
    drop(shared);
    let filter = Filter::new(&root);

    let mut changed: BTreeMap<String, Instant> = BTreeMap::new();
    let mut since: Option<Instant> = None;
    loop {
        let wait = match since {
            Some(start) => MAX_WAIT.saturating_sub(start.elapsed()).min(SETTLE),
            None => Duration::from_secs(3600),
        };
        let received = if wait.is_zero() {
            Err(RecvTimeoutError::Timeout)
        } else {
            rx.recv_timeout(wait)
        };
        match received {
            Ok((at, Ok(event))) => {
                if matches!(event.kind, EventKind::Access(_)) {
                    continue;
                }
                let before = changed.len();
                for path in &event.paths {
                    let Ok(rel) = path.strip_prefix(&root) else {
                        continue;
                    };
                    if !filter.keeps(path, rel) {
                        continue;
                    }
                    let created_dir = matches!(event.kind, EventKind::Create(_)) && path.is_dir();
                    if created_dir && let Some(shared) = watcher.upgrade() {
                        // Files written into a new folder before its watch was in place.
                        for inside in backend::dir_created(&mut lock(&shared), path) {
                            if let Ok(rel) = inside.strip_prefix(&root) {
                                changed.insert(git_path(rel), at);
                            }
                        }
                    }
                    changed.insert(git_path(rel), at);
                }
                // A dropped event queue means anything may have changed.
                if changed.len() > before || event.need_rescan() {
                    since.get_or_insert(at);
                }
            }
            Ok((at, Err(_))) => {
                since.get_or_insert(at);
            }
            Err(RecvTimeoutError::Timeout) => {
                if since.take().is_some() {
                    let burst = std::mem::take(&mut changed);
                    on_change(
                        burst
                            .into_iter()
                            .map(|(path, at)| Change { path, at })
                            .collect(),
                    );
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn lock(watcher: &Mutex<RecommendedWatcher>) -> std::sync::MutexGuard<'_, RecommendedWatcher> {
    watcher.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `rel` as git writes paths, with `/` between parts on every platform.
fn git_path(rel: &Path) -> String {
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

// What the project watcher never watches or reports.

/// Folders never watched, even where no `.gitignore` names them.
const SKIPPED_DIRS: [&str; 3] = [".git", "node_modules", "target"];

/// Decides which event paths are reported.
struct Filter {
    /// The root `.gitignore` and `.git/info/exclude`. Nested ignore files are left out: git
    /// leaves the paths they name out of its status anyway, so missing one costs a refresh, not
    /// a wrong list.
    ignored: Gitignore,
}

impl Filter {
    fn new(root: &Path) -> Self {
        let mut builder = GitignoreBuilder::new(root);
        builder.add(root.join(".gitignore"));
        builder.add(root.join(".git/info/exclude"));
        let ignored = builder.build().unwrap_or_else(|_| Gitignore::empty());
        Self { ignored }
    }

    /// Whether a change to `path`, which is `rel` under the root, is reported. In `.git` its top
    /// level counts, without git's index lock and watchman's cookie files, and so do its branches
    /// and tags, which is all a push changes. Patchwork's own refs for reply snapshots don't.
    fn keeps(&self, path: &Path, rel: &Path) -> bool {
        let mut parts = rel.iter();
        let Some(first) = parts.next() else {
            return false;
        };
        if first == ".git" {
            let Some(name) = parts.next() else {
                return false;
            };
            if name == "refs" {
                let kept = parts
                    .next()
                    .is_some_and(|kind| kind == "heads" || kind == "remotes" || kind == "tags");
                return kept && rel.extension().is_none_or(|ext| ext != "lock");
            }
            return parts.next().is_none() && name != "index.lock" && !is_cookie(name);
        }
        if is_cookie(rel.file_name().unwrap_or_default()) {
            return false;
        }
        let skipped = rel
            .iter()
            .any(|part| SKIPPED_DIRS.iter().any(|dir| part == *dir));
        !skipped
            && !self
                .ignored
                .matched_path_or_any_parents(path, path.is_dir())
                .is_ignore()
    }
}

/// Watchman creates a cookie file in the watched tree whenever it is queried.
fn is_cookie(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with(".watchman-cookie-")
}

/// Walks `dir` the way git sees it, hidden folders included, every ignore file applied.
#[cfg(target_os = "linux")]
fn walker(dir: &Path) -> WalkBuilder {
    let mut walk = WalkBuilder::new(dir);
    walk.hidden(false)
        .filter_entry(|entry| !SKIPPED_DIRS.iter().any(|skip| entry.file_name() == *skip));
    walk
}

/// inotify watches one folder at a time, so each folder git keeps gets its own watch, and
/// ignored folders such as build output cost nothing.
#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};

    use notify::{RecommendedWatcher, RecursiveMode, Watcher};

    use super::walker;

    /// Watches every folder of the working tree that git keeps, the top level of `.git`, and its
    /// branches and tags.
    pub(super) fn watch_project(watcher: &mut RecommendedWatcher, root: &Path) {
        watch_tree(watcher, root);
        // In a linked worktree `.git` is a file, and only the working tree is watched.
        let git_dir = root.join(".git");
        if git_dir.is_dir() {
            let _ = watcher.watch(&git_dir, RecursiveMode::NonRecursive);
            for refs in ["heads", "remotes", "tags"] {
                let _ = watcher.watch(&git_dir.join("refs").join(refs), RecursiveMode::Recursive);
            }
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
}

/// FSEvents and ReadDirectoryChangesW watch a whole tree for the price of one folder, so the
/// root gets one recursive watch and ignored paths are dropped as their events arrive.
#[cfg(not(target_os = "linux"))]
mod tree {
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
}
