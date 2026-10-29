//! Keeps Patchwork up to date from the releases published at updates.patchworkeditor.com. Shortly
//! after launch and then as often as Settings asks, it checks the latest release, downloads this
//! platform's file, checks it against the release's SHA-256 digest and the release signing key's
//! signature, and installs it. The new version
//! runs from the next start; the update card and Settings offer a restart. A copy that was not
//! installed from a release (a dev or source build) cannot replace itself, so it only reports the
//! version.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use patchwork_server::Server;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

mod install;
mod release;
mod schedule;

pub use install::can_install;
pub use schedule::start;
use release::latest_release;

/// The latest release, in the shape of GitHub's release API. The release workflow publishes it.
/// Where updating stands, as the Settings pane shows it.
#[derive(Clone, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    /// A newer release this copy cannot install itself.
    Available {
        version: String,
    },
    Downloading {
        version: String,
        downloaded: u64,
        total: Option<u64>,
    },
    /// Installed (downloaded, on Windows); it runs once Patchwork restarts.
    Ready {
        version: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    /// Whether this copy can install updates itself.
    pub can_install: bool,
    pub state: UpdateState,
    /// The notes of the newer release the last check found, from its section of CHANGELOG.md.
    pub notes: Option<String>,
}

/// What `restart_to_update` did.
#[derive(Serialize)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RestartResult {
    /// Agents are replying in this many tasks; restarting would stop them.
    Busy {
        running: usize,
    },
    /// Nothing is ready to install.
    NothingReady,
    Restarting,
}

#[derive(Default)]
pub struct Updates {
    state: Mutex<Option<UpdateState>>,
    /// One check or download at a time.
    busy: AtomicBool,
    /// Windows installs by running the installer, which closes the app, so the downloaded
    /// installer waits here for a restart or for the app to quit.
    #[cfg_attr(not(windows), allow(dead_code))]
    pending: Mutex<Option<PathBuf>>,
    /// The app is quitting to restart into the update, not because the user quit.
    #[cfg_attr(not(windows), allow(dead_code))]
    restarting: AtomicBool,
    last_check: Mutex<Option<SystemTime>>,
    /// The notes of the newer release the last check found.
    notes: Mutex<Option<String>>,
    /// The user asked for the running check, so it shows its progress and errors.
    watched: AtomicBool,
}

/// Ends the check, even if it panics. A request that came while it ran ends with it.
struct Busy<'a>(&'a Updates);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.watched.store(false, Ordering::SeqCst);
        self.0.busy.store(false, Ordering::SeqCst);
    }
}

impl Updates {
    pub fn status(&self, app: &AppHandle) -> UpdateStatus {
        UpdateStatus {
            current_version: app.package_info().version.to_string(),
            can_install: can_install(),
            state: self.state().unwrap_or(UpdateState::Idle),
            notes: self
                .notes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        }
    }

    fn state(&self) -> Option<UpdateState> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set(&self, app: &AppHandle, state: UpdateState) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = Some(state);
        let _ = app.emit("update-status", self.status(app));
    }

    /// Checks for a newer release, then downloads and installs it. Blocks, so it runs on its own
    /// thread. A check the user asked for shows its progress and its errors; a background one
    /// changes the pane only when it finds something, so going offline does not hide an update
    /// already found.
    pub fn check(&self, app: &AppHandle, manual: bool) {
        if manual {
            self.watched.store(true, Ordering::SeqCst);
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            // A background check is running: it reports to the pane from now on.
            if manual
                && matches!(
                    self.state(),
                    None | Some(UpdateState::UpToDate | UpdateState::Failed { .. })
                )
            {
                self.set(app, UpdateState::Checking);
            }
            return;
        }
        let _busy = Busy(self);
        *self
            .last_check
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(SystemTime::now());
        // An installed update waits for the restart; checking again would download it again.
        if let Some(UpdateState::Ready { .. }) = self.state() {
            return;
        }
        if self.watched.load(Ordering::SeqCst) {
            self.set(app, UpdateState::Checking);
        }
        let release = match latest_release() {
            Ok(release) => release,
            Err(e) => {
                log::warn!("update check failed: {e}");
                if self.watched.swap(false, Ordering::SeqCst) {
                    self.set(app, UpdateState::Failed { message: e });
                }
                return;
            }
        };
        let watched = self.watched.swap(false, Ordering::SeqCst);
        let version = release.tag_name.trim_start_matches('v').to_string();
        let newer = semver::Version::parse(&version)
            .is_ok_and(|latest| latest > app.package_info().version);
        if !newer {
            let shown = self.state();
            if watched || matches!(shown, None | Some(UpdateState::Failed { .. })) {
                self.set(app, UpdateState::UpToDate);
            }
            return;
        }
        *self.notes.lock().unwrap_or_else(PoisonError::into_inner) =
            release.body.clone().filter(|body| !body.trim().is_empty());
        if !can_install() {
            self.set(
                app,
                UpdateState::Available {
                    version: version.clone(),
                },
            );
            return;
        }
        if let Err(message) = self.download_and_install(app, &release, &version) {
            log::warn!("update {version} failed: {message}");
            self.set(app, UpdateState::Failed { message });
            return;
        }
        self.set(
            app,
            UpdateState::Ready {
                version: version.clone(),
            },
        );
    }

    /// Restarts into the installed update, unless agents are replying and `force` is off. The app
    /// quits the usual way.
    pub fn restart(&self, app: &AppHandle, force: bool) -> RestartResult {
        if !matches!(self.state(), Some(UpdateState::Ready { .. })) {
            return RestartResult::NothingReady;
        }
        let running = app.state::<Arc<Server>>().running_turns();
        if running > 0 && !force {
            return RestartResult::Busy { running };
        }
        self.restarting.store(true, Ordering::SeqCst);
        app.request_restart();
        RestartResult::Restarting
    }
}
