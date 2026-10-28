//! Watches a project the way an editor's git view does: the working tree minus what git ignores,
//! plus the top level of `.git` for commits, staging and checkouts. A burst of events is
//! reported once, after it settles, with when each path changed.
//!
//! How folders are watched depends on the platform: inotify watches one folder at a time, so
//! Linux adds a watch per folder git keeps; FSEvents and ReadDirectoryChangesW watch a whole tree
//! for the price of one, so other platforms watch the root and drop ignored paths as they arrive.

mod filter;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(target_os = "linux"))]
mod tree;

#[cfg(target_os = "linux")]
use linux as backend;
#[cfg(not(target_os = "linux"))]
use tree as backend;

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher};

use filter::Filter;

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
            Some(start) if start.elapsed() >= MAX_WAIT => Duration::ZERO,
            Some(start) => SETTLE.min(MAX_WAIT - start.elapsed()),
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
