//! Saving the app's settings, and what the rest of the backend does when they change.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::sync::Arc;

use super::seal::sealed;
use super::types::Settings;
use crate::extensions::skills;
use crate::permissions;
use crate::{Error, Core};

impl Core {
    /// Watches the skills folder in place of any watch before, passing the skills to
    /// `on_change` after each settled burst of changes.
    ///
    /// # Errors
    ///
    /// Returns an error when the skills watcher cannot be created or cannot watch the skills
    /// folder.
    pub fn watch_skills(
        self: &Arc<Self>,
        on_change: impl Fn(Vec<skills::Skill>) + Send + 'static,
    ) -> Result<(), Error> {
        let mut current = self.skills_watch.locked();
        *current = None;
        // The watcher lives in the core, so it holds the core weakly.
        let core = Arc::downgrade(self);
        let watcher = skills::watch(&skills::dir(&self.store.settings()), move || {
            if let Some(core) = core.upgrade() {
                on_change(skills::all(&core.store.settings()));
            }
        })?;
        *current = Some(watcher);
        Ok(())
    }

    pub fn unwatch_skills(&self) {
        *self.skills_watch.locked() = None;
    }

    /// The settings as they are now.
    #[must_use]
    pub fn settings(&self) -> Settings {
        self.store.settings()
    }

    /// Changes the settings with `f` and saves them, returning what `f` returns.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be written to the database.
    pub fn update_settings<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> Result<R, Error> {
        let changed = self.store.update_settings_as(f, sealed)?;
        follow(&self.store.settings());
        Ok(changed)
    }

    /// A value the app keeps under `key`, such as the chat that was open last.
    #[must_use]
    pub fn state(&self, key: &str) -> Option<String> {
        self.store.state(key)
    }

    /// Keeps `value` under `key` in the app state.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub fn set_state(&self, key: &str, value: &str) -> Result<(), Error> {
        self.store.set_state(key, value)
    }

    /// Saves `settings` and applies what changed.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be written to the database.
    pub fn save_settings(self: &Arc<Self>, settings: Settings) -> Result<(), Error> {
        crate::logging::follow(&settings.logging);
        let old = self.store.settings();
        let guards_changed = !permissions::same_guards(&old.permissions, &settings.permissions);
        // The system prompts are what an agent's launch instructions are made of.
        let prompt_changed = old.system_prompts != settings.system_prompts;
        // An agent is handed its skills and hooks as it launches.
        let extensions_changed = old.skills_dir != settings.skills_dir
            || old.disabled_skills != settings.disabled_skills
            || old.hooks_dir != settings.hooks_dir
            || old.disabled_hooks != settings.disabled_hooks;
        // Which chat apps are connected, and how, is what the gateway connects with.
        let gateway_changed = old.gateway.platforms != settings.gateway.platforms;
        let timeout =
            (old.vault.timeout != settings.vault.timeout).then_some(settings.vault.timeout);
        let changed = changed_sections(&old, &settings);
        self.update_settings(|s| *s = settings)?;
        if !changed.is_empty() {
            log::info!(changed:% = changed.join(", "); "settings changed: {{changed}}");
        }
        // A rule the user just added may answer requests already waiting.
        self.runner.apply_rules(self);
        // Idle agents were told the old deny and ask rules, or launched with the old system
        // prompt, skills or hooks, so the next message starts a new one.
        if guards_changed || prompt_changed || extensions_changed {
            self.runner.retire_all_idle();
        }
        // Keeping or dropping the key that opens a vault that never locks waits on the keychain.
        if let Some(timeout) = timeout {
            std::thread::spawn(move || {
                crate::vault::keep_open(timeout == crate::vault::VaultTimeout::Never);
            });
        }
        if gateway_changed {
            let core = Arc::clone(self);
            std::thread::spawn(move || core.restart_gateway());
        }
        Ok(())
    }
}

/// Hands the agent paths and the computer use switch to the code that reads them outside the
/// settings; called whenever the settings are loaded or changed.
pub(crate) fn follow(settings: &Settings) {
    crate::provider::set_agent_paths(&settings.agent_paths);
    crate::computer::set_enabled(settings.computer_use);
}

/// The top-level sections of the settings that differ, for `main.log`. Values are left out, since
/// some hold keys.
fn changed_sections(old: &Settings, new: &Settings) -> Vec<String> {
    let (Ok(serde_json::Value::Object(old)), Ok(serde_json::Value::Object(new))) =
        (serde_json::to_value(old), serde_json::to_value(new))
    else {
        return Vec::new();
    };
    new.iter()
        .filter(|(key, value)| old.get(*key) != Some(value))
        .map(|(key, _)| key.clone())
        .collect()
}
