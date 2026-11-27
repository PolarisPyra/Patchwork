//! Wires [`patchwork_daemon::updates`] and [`patchwork_daemon::restart`] into [`Core`].

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

pub use patchwork_daemon::updates::{
    RELEASES_URL, ReleaseNotes, RestartResult, UpdateSettings, UpdateState, UpdateStatus, Updates,
    notes, release_helper, restart_path,
};

use crate::settings::ResumeAtLaunch;
use crate::{Core, Error};

/// The saved version whose update card the user closed.
const DISMISSED_KEY: &str = "update-dismissed";

impl Core {
    /// Where updating stands.
    pub fn update_status(&self) -> UpdateStatus {
        self.updates.status()
    }

    /// Checks for an update because the user asked, on a thread of its own, and installs what it
    /// finds. `update_status` shows its progress and its errors.
    pub fn check_for_updates(self: &Arc<Self>) {
        self.updates.spawn_check();
    }

    /// Whether the app may restart into the installed update. Replies that are running are cut
    /// off by the restart and resumed after it, so it only holds back, unless `force` is on, when
    /// Settings says a launch never resumes them.
    pub fn restart_to_update(&self, force: bool) -> RestartResult {
        let resumes = self.store.settings().resume_at_launch != ResumeAtLaunch::Never;
        self.updates
            .restart_readiness(self.running_turns(), force, resumes)
    }

    /// Starts the background checks, for the life of the app. A dev build runs its checkout's
    /// version, which is behind the latest release until it is pulled, so it never checks on its
    /// own; Settings can still check.
    pub fn start_update_checks(self: &Arc<Self>) {
        let core = Arc::clone(self);
        self.updates
            .start_background_checks(move || core.store.settings().updates.check_every_minutes);
    }

    /// The version whose update card the user closed, so each version's card shows once.
    pub fn dismissed_update(&self) -> Option<String> {
        self.store.state(DISMISSED_KEY)
    }

    /// Remembers that the user closed the update card of `version`.
    ///
    /// # Errors
    ///
    /// Returns an error when it cannot be saved.
    pub fn dismiss_update(&self, version: &str) -> Result<(), Error> {
        self.store.set_state(DISMISSED_KEY, version)
    }
}
