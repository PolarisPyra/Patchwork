//! Watching the theme folders, so an edited file shows without a restart.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;

use notify::RecursiveMode;

use super::library::{CODE, SCENES, THEMES};
use crate::workspace::watch::settled;
use crate::{Error, Core};

/// Watches the theme folders until dropped.
#[derive(Debug)]
pub struct Watcher {
    _watcher: notify::RecommendedWatcher,
}

impl Core {
    /// Watches the themes, scenes and code themes folders in place of any watch before, and
    /// runs `on_change` on a thread of its own once each burst of changes settles.
    ///
    /// # Errors
    ///
    /// Returns an error when the watcher cannot be created, cannot watch a folder, or its
    /// thread cannot be started.
    pub fn watch_themes(&self, on_change: impl Fn() + Send + 'static) -> Result<(), Error> {
        let mut current = self.themes_watch.locked();
        *current = None;
        let dirs = [THEMES.dir(), SCENES.dir(), CODE.dir()];
        let watcher = settled(
            "themes-watch",
            &dirs,
            RecursiveMode::NonRecursive,
            on_change,
        )?;
        *current = Some(Watcher { _watcher: watcher });
        Ok(())
    }
}
