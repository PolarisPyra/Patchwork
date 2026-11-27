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

    /// Changes the settings with `f`, saves them and applies what changed.
    ///
    /// This is the one way the settings change, so every change reaches the logging, the
    /// permission rules, idle agents, the vault and the chat apps that follow them.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be written to the database.
    pub fn update_settings<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> Result<R, Error> {
        let old = self.store.settings();
        let result = self.store.update_settings_as(f, sealed)?;
        self.settings_changed(&old, &self.store.settings());
        Ok(result)
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

    /// Applies what changed between `old` and `new` to the rest of the backend.
    fn settings_changed(&self, old: &Settings, new: &Settings) {
        follow(new);
        crate::logging::follow(&new.logging);
        let changed = changed_sections(old, new);
        if changed.is_empty() {
            return;
        }
        log::info!(changed:% = changed.join(", "); "settings changed: {{changed}}");
        let core = self.me.upgrade();
        // A rule the user just added may answer requests already waiting.
        if let Some(core) = &core {
            self.runner.apply_rules(core);
        }
        let guards_changed = !permissions::same_guards(&old.permissions, &new.permissions);
        // The system prompts are what an agent's launch instructions are made of.
        let prompt_changed = old.system_prompts != new.system_prompts;
        // An agent is handed its skills and hooks as it launches.
        let extensions_changed = old.skills_dir != new.skills_dir
            || old.disabled_skills != new.disabled_skills
            || old.hooks_dir != new.hooks_dir
            || old.disabled_hooks != new.disabled_hooks;
        // Idle agents were told the old deny and ask rules, or launched with the old system
        // prompt, skills or hooks, so the next message starts a new one.
        if guards_changed || prompt_changed || extensions_changed {
            self.runner.retire_all_idle();
        }
        // Keeping or dropping the key that opens a vault that never locks waits on the keychain.
        if old.vault.timeout != new.vault.timeout {
            let never = new.vault.timeout == crate::vault::VaultTimeout::Never;
            std::thread::spawn(move || crate::vault::keep_open(never));
        }
        // Which chat apps are connected, and how, is what the gateway connects with.
        if old.gateway.platforms != new.gateway.platforms
            && let Some(core) = core
        {
            std::thread::spawn(move || core.restart_gateway());
        }
    }
}

/// Hands the agent paths and the computer use and screen switches to the code that reads them
/// outside the settings; called whenever the settings are loaded or changed.
pub(crate) fn follow(settings: &Settings) {
    crate::provider::set_agent_paths(&settings.agent_paths);
    crate::computer::set_enabled(settings.computer_use);
    crate::computer::set_seeing(settings.see_screen);
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

#[cfg(test)]
mod tests {
    use super::{Settings, changed_sections};
    use crate::Core;

    #[test]
    fn changed_sections_names_the_keys_that_differ() {
        let old = Settings::default();
        let mut new = old.clone();
        new.window.active_project_id = Some("p-1".into());
        new.show_thoughts = true;
        let mut changed = changed_sections(&old, &new);
        changed.sort();
        assert_eq!(changed, ["activeProjectId", "showThoughts"]);
        assert!(changed_sections(&old, &old).is_empty());
    }

    #[test]
    fn settings_update_saves_through_the_core() {
        let dir = std::env::temp_dir().join(format!("patchwork-save-{}", crate::system::new_id()));
        std::fs::create_dir_all(&dir).expect("the temporary folder is made");
        let core = Core::for_tests(&dir, Box::new(|_| {}));
        core.update_settings(|s| s.show_thoughts = true)
            .expect("the switch saves");
        assert!(core.settings().show_thoughts);
        let kept = core.update_settings(|s| std::mem::take(&mut s.show_thoughts));
        assert!(kept.expect("the settings save"));
        assert!(!core.settings().show_thoughts);
        drop(core);
        let _ = std::fs::remove_dir_all(dir);
    }
}
