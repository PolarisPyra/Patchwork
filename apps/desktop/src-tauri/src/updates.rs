//! Keeps Patchwork up to date from its GitHub releases. Shortly after launch and then every hour
//! it checks the latest release, downloads this platform's file, checks it against the SHA-256
//! digest GitHub publishes for it, and installs it, notifying once per version. The new version
//! runs from the next start; the update card and Settings offer a restart. A copy that was not
//! installed from a release (a dev or source build) cannot replace itself, so it only reports the
//! version.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use patchwork_server::Server;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/PolarisPyra/Patchwork-ADE/releases/latest";
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
    /// Windows installs by running the installer, which closes the app, so the downloaded
    /// installer waits here for a restart or for the app to quit.
    #[cfg_attr(not(windows), allow(dead_code))]
    pending: Mutex<Option<PathBuf>>,
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

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// `sha256:<hex>`, published by GitHub for each asset.
    digest: Option<String>,
}

/// The release file this copy installs from: the AppImage, the Windows installer, or the macOS
/// app bundle, as the release workflow names them.
fn is_own_asset(name: &str) -> bool {
    if cfg!(windows) {
        name.ends_with("-setup.exe")
    } else if cfg!(target_os = "macos") {
        name == "Patchwork.app.tar.gz"
    } else {
        name.ends_with(".AppImage")
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
        Some(BundleType::App) => app_bundle().is_some(),
        _ => false,
    }
}

/// The `.app` bundle this copy runs from, on macOS.
fn app_bundle() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    // Patchwork.app/Contents/MacOS/patchwork
    let bundle = exe.parent()?.parent()?.parent()?;
    (bundle.extension()? == "app").then(|| bundle.to_path_buf())
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(concat!("Patchwork/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
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
        let newer = semver::Version::parse(&version).is_ok_and(|latest| latest > app.package_info().version);
        if !newer {
            let shown = self.state();
            if watched || matches!(shown, None | Some(UpdateState::Failed { .. })) {
                self.set(app, UpdateState::UpToDate);
            }
            return;
        }
        if !can_install() {
            self.set(app, UpdateState::Available { version: version.clone() });
            notify_once(app, &version, "Download it from the releases page.");
            return;
        }
        notify_once(app, &version, "Downloading it now. It installs on its own.");
        if let Err(message) = self.download_and_install(app, &release, &version) {
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

    fn download_and_install(&self, app: &AppHandle, release: &Release, version: &str) -> Result<(), String> {
        let asset = release
            .assets
            .iter()
            .find(|asset| is_own_asset(&asset.name))
            .ok_or("The release has no download for this system.")?;
        let sha256 = asset
            .digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .ok_or("The release does not publish this download's checksum.")?;
        let file = self.download(app, &asset.browser_download_url, sha256, version)?;
        install(file, self)
    }

    /// Downloads `url` into the temporary folder and checks its SHA-256.
    fn download(&self, app: &AppHandle, url: &str, sha256: &str, version: &str) -> Result<PathBuf, String> {
        self.set(app, UpdateState::Downloading { version: version.into(), downloaded: 0, total: None });
        let mut response = agent(DOWNLOAD_TIMEOUT)
            .get(url)
            .call()
            .map_err(|e| format!("The download did not start: {e}"))?;
        let total = response.body().content_length();
        let name = url.rsplit('/').next().unwrap_or("patchwork-update");
        let path = std::env::temp_dir().join(format!("patchwork-{version}-{name}"));
        let result = (|| {
            let mut file = std::fs::File::create(&path).map_err(|e| format!("Could not save the download: {e}"))?;
            let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
            let mut hasher = Sha256::new();
            let mut buf = vec![0u8; 1 << 16];
            let (mut downloaded, mut shown) = (0u64, 0u64);
            // Each whole percent, or each MiB when the size is unknown.
            let step = total.map_or(1 << 20, |t| (t / 100).max(1));
            loop {
                let read = reader.read(&mut buf).map_err(|e| format!("The download stopped: {e}"))?;
                if read == 0 {
                    break;
                }
                hasher.update(&buf[..read]);
                file.write_all(&buf[..read]).map_err(|e| format!("Could not save the download: {e}"))?;
                downloaded += read as u64;
                if downloaded / step != shown / step {
                    shown = downloaded;
                    self.set(app, UpdateState::Downloading { version: version.into(), downloaded, total });
                }
            }
            file.sync_all().map_err(|e| e.to_string())?;
            let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
            if !got.eq_ignore_ascii_case(sha256) {
                return Err("The download does not match its checksum.".to_string());
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(path),
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                Err(e)
            }
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

    /// Runs a downloaded Windows installer as the app quits, once the agents have stopped. The
    /// installer closes the app, and starts it again when this quit is a restart.
    pub fn install_on_exit(&self) {
        #[cfg(windows)]
        if let Some(installer) = self.pending.lock().unwrap_or_else(PoisonError::into_inner).take() {
            // Passive mode with a progress bar, as an update, like the installer's own updates.
            let mut cmd = std::process::Command::new(&installer);
            cmd.args(["/P", "/UPDATE"]);
            if self.restarting.load(Ordering::SeqCst) {
                cmd.arg("/R");
            }
            match cmd.spawn() {
                Ok(_) => std::process::exit(0),
                Err(e) => log::warn!("installing the update failed: {e}"),
            }
        }
    }
}

fn latest_release() -> Result<Release, String> {
    let mut response = agent(CHECK_TIMEOUT)
        .get(LATEST_RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;
    response
        .body_mut()
        .read_json()
        .map_err(|e| format!("GitHub sent an unreadable release: {e}"))
}

/// Keeps the verified installer for the restart or the quit, since running it closes the app.
#[cfg(windows)]
fn install(file: PathBuf, updates: &Updates) -> Result<(), String> {
    *updates.pending.lock().unwrap_or_else(PoisonError::into_inner) = Some(file);
    Ok(())
}

/// Swaps the app bundle for the verified one.
#[cfg(target_os = "macos")]
fn install(file: PathBuf, _updates: &Updates) -> Result<(), String> {
    let result = install_bundle(&file);
    let _ = std::fs::remove_file(&file);
    result
}

/// Moves the verified AppImage over the running one, which keeps its open file until the restart.
#[cfg(all(unix, not(target_os = "macos")))]
fn install(file: PathBuf, _updates: &Updates) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let target = PathBuf::from(std::env::var_os("APPIMAGE").ok_or("Not running as an AppImage.")?);
    // Copied beside the AppImage first, so the move into place is a rename on one filesystem.
    let staged = target.with_extension("download");
    let result = std::fs::copy(&file, &staged)
        .and_then(|_| std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)))
        .and_then(|()| std::fs::rename(&staged, &target))
        .map_err(|e| format!("Could not replace {}: {e}", target.display()));
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    let _ = std::fs::remove_file(&file);
    result
}

/// Unpacks the new app bundle beside the running one, then swaps them. A bundle in a folder the
/// user cannot write to is not replaced.
#[cfg(target_os = "macos")]
fn install_bundle(archive: &std::path::Path) -> Result<(), String> {
    let bundle = app_bundle().ok_or("Not running from an app bundle.")?;
    let parent = bundle.parent().ok_or("The app bundle has no folder.")?;
    let staging = parent.join(".patchwork-update");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| {
        format!("Patchwork cannot write to {}, so download the update from the releases page: {e}", parent.display())
    })?;
    let result = (|| {
        let unpacked = std::process::Command::new("tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(&staging)
            .status()
            .map_err(|e| format!("Could not unpack the update: {e}"))?;
        if !unpacked.success() {
            return Err("Could not unpack the update.".to_string());
        }
        let new = staging.join("Patchwork.app");
        if !new.is_dir() {
            return Err("The update has no Patchwork.app.".to_string());
        }
        let old = staging.join("previous.app");
        std::fs::rename(&bundle, &old).map_err(|e| format!("Could not replace {}: {e}", bundle.display()))?;
        if let Err(e) = std::fs::rename(&new, &bundle) {
            let _ = std::fs::rename(&old, &bundle);
            return Err(format!("Could not replace {}: {e}", bundle.display()));
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
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
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        loop {
            let handle = app.clone();
            if handle.state::<Updates>().due() {
                // In its own thread, so a panic in one check does not end the checks.
                let check = std::thread::spawn(move || handle.state::<Updates>().check(&handle, false));
                if check.join().is_err() {
                    log::warn!("update check stopped");
                }
            }
            std::thread::sleep(TICK);
        }
    });
}
