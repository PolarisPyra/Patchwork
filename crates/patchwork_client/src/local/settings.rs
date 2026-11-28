//! The app's settings in the desktop store.

// Rust guideline compliant 2026-10-09

use std::ops::Deref;
use std::sync::Arc;

use patchwork_core::Core;
use patchwork_core::settings::Settings;

use super::AppState;

/// The app's settings, as last saved.
#[derive(Debug, Clone)]
pub struct SettingsStore(Arc<Settings>);

impl SettingsStore {
    pub(super) fn read(core: &Core) -> Self {
        Self(Arc::new(core.settings()))
    }

    /// The settings, shared.
    #[must_use]
    pub fn shared(&self) -> Arc<Settings> {
        Arc::clone(&self.0)
    }

    /// The settings, to change in place before they are saved.
    pub fn make_mut(&mut self) -> &mut Settings {
        Arc::make_mut(&mut self.0)
    }
}

impl Deref for SettingsStore {
    type Target = Settings;

    fn deref(&self) -> &Settings {
        &self.0
    }
}

impl AppState {
    /// Reads the settings again, after a change to them.
    pub fn reload_settings(&mut self) {
        self.settings = SettingsStore::read(&self.core);
    }
}
