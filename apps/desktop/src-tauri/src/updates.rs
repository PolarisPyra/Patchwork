//! Keeps Patchwork up to date from its GitHub releases. Shortly after launch and then every hour
//! it checks for a newer release, downloads and installs it, and notifies once per version. The
//! new version runs from the next start; Settings offers a restart. A copy that was not installed
//! from a release (a dev or source build) cannot replace itself, so it only reports the version.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use patchwork_server::Server;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::{Error, Update, UpdaterExt};

/// How long after launch the first check waits, so it does not slow the start.
const FIRST_CHECK: Duration = Duration::from_secs(30);
/// How often to check. Measured on the wall clock, so time spent asleep counts.
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
/// How often the timer looks at the clock.
const TICK: Duration = Duration::from_secs(5 * 60);
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// A download that has not finished by then has stalled; the next check starts it again.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// The last version the user was told about, so each one notifies once.
const NOTIFIED_KEY: &str = "update-notified-version";

/// Where updating stands, as the Settings pane shows it.
#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    /// A newer release this copy cannot install itself.
    Available { version: String },
    Downloading { version: String, downloaded: u64, total: Option<u64> },
    /// Installed (downloaded, on Windows); it runs once Patchwork restarts.
    Ready { version: String },
    Failed { message: String },
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    /// Whether this copy can install updates itself.
    pub can_install: bool,
    pub state: UpdateState,
}

/// What `restart_to_update` did.
#[derive(Serialize)]
#[serde(tag = "result", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RestartResult {
    /// Agents are replying in this many tasks; restarting would stop them.
    Busy { running: usize },
    /// Nothing is ready to install.
    NothingReady,
    Restarting,
}

#[derive(Default)]
pub struct Updates {
    state: Mutex<Option<UpdateState>>,
    /// One check or download at a time.
    busy: AtomicBool,
    /// Windows installs by running the installer, which closes the app, so the download waits
    /// here for a restart or for the app to quit.
    #[cfg_attr(not(windows), allow(dead_code))]
    pending: Mutex<Option<(Update, Vec<u8>)>>,
    /// The app is quitting to restart into the update, not because the user quit.
    #[cfg_attr(not(windows), allow(dead_code))]
    restarting: AtomicBool,
    last_check: Mutex<Option<SystemTime>>,
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

/// Whether this copy is a release bundle the updater can replace. A build with no bundle would
/// have its binary overwritten with an AppImage, so only the bundles the releases ship qualify.
pub fn can_install() -> bool {
    use tauri::utils::config::BundleType;
    if cfg!(debug_assertions) {
        return false;
    }
    match tauri::utils::platform::bundle_type() {
        Some(BundleType::AppImage) => std::env::var_os("APPIMAGE").is_some(),
        Some(BundleType::Nsis) => true,
        Some(BundleType::App) => cfg!(target_os = "macos")
            && std::env::current_exe()
                .is_ok_and(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS/")),
        _ => false,
    }
}

impl Updates {
    pub fn status(&self, app: &AppHandle) -> UpdateStatus {
        UpdateStatus {
            current_version: app.package_info().version.to_string(),
            can_install: can_install(),
            state: self.state().unwrap_or(UpdateState::Idle),
        }
    }

    fn state(&self) -> Option<UpdateState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn set(&self, app: &AppHandle, state: UpdateState) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = Some(state);
        let _ = app.emit("update-status", self.status(app));
    }

    /// Checks for a newer release, then downloads and installs it. A check the user asked for
    /// shows its progress and its errors; a background one changes the pane only when it finds
    /// something, so going offline does not hide an update already found.
    pub async fn check(&self, app: &AppHandle, manual: bool) {
        if manual {
            self.watched.store(true, Ordering::SeqCst);
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            // A background check is running: it reports to the pane from now on.
            if manual && matches!(self.state(), None | Some(UpdateState::UpToDate | UpdateState::Failed { .. })) {
                self.set(app, UpdateState::Checking);
            }
            return;
        }
        let _busy = Busy(self);
        *self.last_check.lock().unwrap_or_else(PoisonError::into_inner) = Some(SystemTime::now());
        // An installed update waits for the restart; checking again would download it again.
        if let Some(UpdateState::Ready { .. }) = self.state() {
            return;
        }
        if self.watched.load(Ordering::SeqCst) {
            self.set(app, UpdateState::Checking);
        }
        let found = match app.updater_builder().timeout(CHECK_TIMEOUT).build() {
            Ok(updater) => updater.check().await,
            Err(e) => Err(e),
        };
        let watched = self.watched.swap(false, Ordering::SeqCst);
        let update = match found {
            Ok(Some(update)) => update,
            // A release without an update manifest has nothing to update to.
            Ok(None) | Err(Error::ReleaseNotFound) => {
                let shown = self.state();
                if watched || matches!(shown, None | Some(UpdateState::Failed { .. })) {
                    self.set(app, UpdateState::UpToDate);
                }
                return;
            }
            Err(e) => {
                log::warn!("update check failed: {e}");
                if watched {
                    self.set(app, UpdateState::Failed { message: e.to_string() });
                }
                return;
            }
        };
        let version = update.version.clone();
        if !can_install() {
            self.set(app, UpdateState::Available { version: version.clone() });
            notify_once(app, &version, "Download it from the releases page.");
            return;
        }
        notify_once(app, &version, "Downloading it now. It installs on its own.");
        if let Err(message) = self.download_and_install(app, update).await {
            log::warn!("update {version} failed: {message}");
            self.set(app, UpdateState::Failed { message });
            return;
        }
        self.set(app, UpdateState::Ready { version: version.clone() });
        let _ = app
            .notification()
            .builder()
            .title(format!("Patchwork {version} is installed"))
            .body("Restart Patchwork to use it.")
            .show();
    }

    async fn download_and_install(&self, app: &AppHandle, update: Update) -> Result<(), String> {
        let version = update.version.clone();
        self.set(app, UpdateState::Downloading { version: version.clone(), downloaded: 0, total: None });
        let mut downloaded = 0u64;
        let mut shown = 0u64;
        let download = update.download(
            |chunk, total| {
                downloaded += chunk as u64;
                // Each whole percent, or each MiB when the size is unknown.
                let step = total.map_or(1 << 20, |t| (t / 100).max(1));
                if downloaded / step != shown / step {
                    shown = downloaded;
                    self.set(app, UpdateState::Downloading { version: version.clone(), downloaded, total });
                }
            },
            || {},
        );
        let bytes = tokio::time::timeout(DOWNLOAD_TIMEOUT, download)
            .await
            .map_err(|_| "The download stalled. Patchwork will try again later.".to_string())?
            .map_err(|e| e.to_string())?;
        #[cfg(windows)]
        {
            *self.pending.lock().unwrap_or_else(PoisonError::into_inner) = Some((update, bytes));
            Ok(())
        }
        #[cfg(not(windows))]
        {
            // Writes the new bundle beside the running one, then moves it into place.
            tauri::async_runtime::spawn_blocking(move || update.install(bytes))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())
        }
    }

    /// Whether an hour has passed since the last check.
    fn due(&self) -> bool {
        let last = *self.last_check.lock().unwrap_or_else(PoisonError::into_inner);
        last.is_none_or(|last| last.elapsed().map_or(true, |since| since >= CHECK_EVERY))
    }

    /// Restarts into the installed update, unless agents are replying and `force` is off. The app
    /// quits the usual way, which records their turns as interrupted, so they carry on after it.
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

    /// Runs a downloaded Windows update's installer as the app quits, once the agents have
    /// stopped. The installer exits the app, and starts it again when this quit is a restart.
    pub fn install_on_exit(&self) {
        #[cfg(windows)]
        if let Some((update, bytes)) = self.pending.lock().unwrap_or_else(PoisonError::into_inner).take() {
            let restart = self.restarting.load(Ordering::SeqCst);
            if let Err(e) = update.restart_after_install(restart).install(bytes) {
                log::warn!("installing the update failed: {e}");
            }
        }
    }
}

/// Tells the user about a new version, once per version.
fn notify_once(app: &AppHandle, version: &str, body: &str) {
    let server = app.state::<Arc<Server>>();
    if server.store.state(NOTIFIED_KEY).as_deref() == Some(version) {
        return;
    }
    if let Err(e) = server.store.set_state(NOTIFIED_KEY, version) {
        log::warn!("could not record the update notification: {e}");
    }
    let _ = app
        .notification()
        .builder()
        .title(format!("Patchwork {version} is available"))
        .body(body)
        .show();
}

/// Starts the background checks.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            if app.state::<Updates>().due() {
                // In its own task, so a panic in one check does not end the checks.
                let handle = app.clone();
                let check = tauri::async_runtime::spawn(async move {
                    handle.state::<Updates>().check(&handle, false).await;
                });
                if let Err(e) = check.await {
                    log::warn!("update check stopped: {e}");
                }
            }
            tokio::time::sleep(TICK).await;
        }
    });
}
