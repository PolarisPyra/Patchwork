//! What the Android tools read from the app that runs them: the user's settings, the app's saved
//! state, and the project a chat works in.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use patchwork_platform::Error;

use super::emulator::Emulators;

/// The app the Android tools run in.
pub trait Host: Sync {
    /// The SDK folder the user typed in Settings, if any.
    fn sdk_setting(&self) -> Option<String>;

    /// Whether the user lets agents use Android devices.
    fn agents_allowed(&self) -> bool;

    /// The value the app keeps under `key`.
    fn state(&self, key: &str) -> Option<String>;

    /// Keeps `value` under `key`.
    ///
    /// # Errors
    ///
    /// Returns an error when the app's state cannot be saved.
    fn set_state(&self, key: &str, value: &str) -> Result<(), Error>;

    /// The project a chat works in, by id, and its folder. None when the chat has none.
    fn chat_project(&self, chat_id: &str) -> Option<(String, PathBuf)>;
}

/// The Android tools, acting for `host` on the emulators it started.
#[derive(Clone, Copy)]
pub struct Android<'a> {
    pub(crate) host: &'a dyn Host,
    pub(crate) emulators: &'a Emulators,
}

impl<'a> Android<'a> {
    /// The tools for `host`, which keeps `emulators` for as long as it runs.
    #[must_use]
    pub fn new(host: &'a dyn Host, emulators: &'a Emulators) -> Self {
        Self { host, emulators }
    }
}

impl std::fmt::Debug for Android<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Android").finish_non_exhaustive()
    }
}
