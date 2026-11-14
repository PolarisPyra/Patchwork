//! Watching the theme folders, so an edited file shows without a restart.

// Rust guideline compliant 2026-10-09

use std::sync::PoisonError;
use std::time::Duration;

use notify::{RecursiveMode, Watcher as _};

use super::library::{CODE, SCENES, THEMES};
use crate::{Error, Server};

/// Watches the theme folders until dropped.
#[derive(Debug)]
pub struct Watcher {
    _watcher: notify::RecommendedWatcher,
}

/// Quiet time that ends a burst of changes: an editor's save is several.
const SETTLE: Duration = Duration::from_millis(200);

impl Server {
    /// Watches the themes, scenes and code themes folders in place of any watch before, and
    /// runs `on_change` on a thread of its own once each burst of changes settles.
    ///
    /// # Errors
    ///
    /// Returns an error when the watcher cannot be created, cannot watch a folder, or its
    /// thread cannot be started.
    pub fn watch_themes(&self, on_change: impl Fn() + Send + 'static) -> Result<(), Error> {
        let mut current = self
            .themes_watch
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *current = None;
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if !res.is_ok_and(|e| matches!(e.kind, notify::EventKind::Access(_))) {
                let _ = tx.send(());
            }
        })
        .map_err(|e| e.to_string())?;
        for dir in [THEMES.dir(), SCENES.dir(), CODE.dir()] {
            watcher
                .watch(&dir, RecursiveMode::NonRecursive)
                .map_err(|e| format!("Could not watch {}: {e}", dir.display()))?;
        }
        // Ends once the watcher is dropped, which closes the channel.
        std::thread::Builder::new()
            .name("themes-watch".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    while rx.recv_timeout(SETTLE).is_ok() {}
                    on_change();
                }
            })
            .map_err(|e| e.to_string())?;
        *current = Some(Watcher { _watcher: watcher });
        Ok(())
    }
}
