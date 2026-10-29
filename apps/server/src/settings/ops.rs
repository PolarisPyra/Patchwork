//! Saving settings, and watching the skills folder while Settings shows it.

use std::sync::Arc;

use crate::settings::Settings;
use crate::{permissions, skills};
use crate::follow_log_levels;
use crate::Server;

impl Server {
    /// Watches the skills folder in place of any watch before, passing the skills to
    /// `on_change` after each settled burst of changes.
    pub fn watch_skills(
        self: &Arc<Self>,
        on_change: impl Fn(Vec<skills::Skill>) + Send + 'static,
    ) -> Result<(), String> {
        let mut current = self
            .skills_watch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current = None;
        // The watcher lives in the server, so it holds the server weakly.
        let server = Arc::downgrade(self);
        let watcher = skills::watch(&skills::dir(&self.store.settings()), move || {
            if let Some(server) = server.upgrade() {
                on_change(skills::all(&server.store.settings()));
            }
        })?;
        *current = Some(watcher);
        Ok(())
    }

    pub fn unwatch_skills(&self) {
        *self
            .skills_watch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    pub fn save_settings(&self, settings: Settings) -> Result<(), String> {
        follow_log_levels(&settings);
        let guards_changed =
            !permissions::same_guards(&self.store.settings().permissions, &settings.permissions);
        self.store.update_settings(|s| *s = settings)?;
        // A rule the user just added may answer requests already waiting.
        self.runner.apply_rules(self);
        // Idle agents were told the old deny and ask rules, so the next message starts a new one.
        if guards_changed {
            self.runner.retire_all_idle();
        }
        Ok(())
    }
}
