//! Finding the Chrome-family browser installed on this computer and starting it headless, with
//! the `DevTools` protocol on a local port.
//!
//! Each start gets a profile folder of its own, so two pages open at once never fight over one
//! profile's lock; the folder goes once the browser has exited. On Linux the browser is killed
//! with Patchwork, and folders a killed browser left are removed at the next start.
//!
//! Starting a browser takes most of the time a page takes to show, so one is kept started ahead
//! of need while pages are shown: a page takes it and another starts behind it. Once no page is
//! shown, the spare is stopped, so an idle Patchwork runs no browser.

// Rust guideline compliant 2026-10-09

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError, mpsc};
use std::time::Duration;

use crate::Error;

/// The browsers looked for on `PATH`, in order: Google Chrome first, as most people have it, then
/// Chromium and the browsers built on it.
const BROWSERS: &[&str] = &[
    "google-chrome-stable",
    "google-chrome",
    "chromium",
    "chromium-browser",
    "brave-browser",
    "brave",
    "microsoft-edge-stable",
    "microsoft-edge",
    "vivaldi-stable",
    "vivaldi",
    "helium-browser",
    "chrome",
];

/// Where the browser is installed on macOS and Windows, where it is seldom on `PATH`.
const INSTALLED: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
];

/// How long the browser has to start and print its `DevTools` address. A cold start with a new
/// profile takes one to two seconds.
const START_WAIT: Duration = Duration::from_secs(20);

/// The line the browser prints its `DevTools` address after.
const LISTENING: &str = "DevTools listening on ";

/// The installed browser Patchwork shows pages with, if any.
#[must_use]
pub fn find_browser() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let on_path = BROWSERS.iter().find_map(|name| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|file| file.is_file())
    });
    on_path.or_else(|| {
        INSTALLED
            .iter()
            .map(PathBuf::from)
            .find(|file| file.is_file())
    })
}

/// A headless browser this session started.
#[derive(Debug)]
pub(super) struct Launched {
    pub child: Child,
    /// The browser's `DevTools` websocket.
    pub address: String,
    /// The profile folder made for it, removed once it exits.
    pub profile: PathBuf,
}

/// The browser started ahead of need, when one is ready.
static SPARE: Mutex<Option<Launched>> = Mutex::new(None);

/// Whether a spare is wanted: from [`warm_browser`] until [`cool_browser`]. Changed only while
/// [`SPARE`] is locked, so a spare that finishes starting after it is no longer wanted is stopped.
static WANTED: AtomicBool = AtomicBool::new(false);

/// Asks the thread that keeps [`SPARE`] filled to start a browser.
static WARMER: OnceLock<mpsc::Sender<()>> = OnceLock::new();

/// Starts a browser in the background, unless one is ready, so the next page shows without
/// waiting for one.
pub fn warm_browser() {
    if find_browser().is_none() {
        return;
    }
    // Under the spare's lock, so the warmer and a cooling see it in order.
    let spare = SPARE.lock().unwrap_or_else(PoisonError::into_inner);
    WANTED.store(true, Ordering::Relaxed);
    drop(spare);
    let warmer = WARMER.get_or_init(|| {
        let (ask, asked) = mpsc::channel::<()>();
        // A browser dies with the thread that started it, so this thread lives as long as the app.
        let spawned = std::thread::Builder::new()
            .name("browser-spare".into())
            .spawn(move || {
                while asked.recv().is_ok() {
                    {
                        let spare = SPARE.lock().unwrap_or_else(PoisonError::into_inner);
                        if spare.is_some() || !WANTED.load(Ordering::Relaxed) {
                            continue;
                        }
                    }
                    match launch() {
                        Ok(launched) => {
                            let mut spare = SPARE.lock().unwrap_or_else(PoisonError::into_inner);
                            if WANTED.load(Ordering::Relaxed) {
                                *spare = Some(launched);
                            } else {
                                drop(spare);
                                reap(launched.child, &launched.profile, Duration::ZERO);
                            }
                        }
                        Err(e) => log::warn!(error:% = e; "no spare browser: {{error}}"),
                    }
                }
            });
        if let Err(e) = spawned {
            log::warn!(error:% = e; "the spare browser's thread did not start: {{error}}");
        }
        ask
    });
    let _ = warmer.send(());
}

/// Stops the spare browser, and any that is starting, once no page is shown. Blocks for the
/// moment the browser takes to exit, so it runs off the UI thread.
pub fn cool_browser() {
    let spare = {
        let mut spare = SPARE.lock().unwrap_or_else(PoisonError::into_inner);
        WANTED.store(false, Ordering::Relaxed);
        spare.take()
    };
    if let Some(spare) = spare {
        reap(spare.child, &spare.profile, Duration::ZERO);
    }
}

/// The spare browser if it is still running, or one started now, and another spare started
/// behind it.
///
/// # Errors
///
/// Returns an error when no browser is installed, or it does not start.
pub(super) fn take_or_launch() -> Result<Launched, Error> {
    let spare = SPARE.lock().unwrap_or_else(PoisonError::into_inner).take();
    warm_browser();
    let Some(mut spare) = spare else {
        return launch();
    };
    if matches!(spare.child.try_wait(), Ok(None)) {
        return Ok(spare);
    }
    reap(spare.child, &spare.profile, Duration::ZERO);
    launch()
}

/// Starts the browser headless and waits for its `DevTools` address.
///
/// # Errors
///
/// Returns an error when no browser is installed, or it does not start.
fn launch() -> Result<Launched, Error> {
    let browser = find_browser().ok_or_else(|| {
        Error::Message("Showing pages needs Google Chrome or another Chromium browser.".into())
    })?;
    let root = crate::system::dirs().cache.join("browser");
    let profile = root.join(format!("profile-{}", uuid::Uuid::new_v4()));
    remove_stale_profiles(&root);
    std::fs::create_dir_all(&profile)?;
    // The browser's own messages, kept for when a page does not show.
    let mut log = std::fs::File::create(root.join("browser.log"))?;
    let mut command = Command::new(&browser);
    die_with_patchwork(&mut command);
    let mut child = command
        .arg("--headless")
        .arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", profile.display()))
        .args([
            "--no-first-run",
            "--no-default-browser-check",
            "--mute-audio",
            "--hide-crash-restore-bubble",
            "about:blank",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Message(format!("Could not start {}: {e}", browser.display())))?;
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        return Err(Error::Message("The browser's output did not open".into()));
    };

    let (found, address) = mpsc::channel();
    std::thread::Builder::new()
        .name("browser-log".into())
        .spawn(move || {
            let mut found = Some(found);
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(rest) = line.strip_prefix(LISTENING)
                    && let Some(found) = found.take()
                {
                    let _ = found.send(rest.trim().to_owned());
                }
                let _ = writeln!(log, "{line}");
            }
        })?;

    if let Ok(address) = address.recv_timeout(START_WAIT) {
        return Ok(Launched {
            child,
            address,
            profile,
        });
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&profile);
    Err(Error::Message(format!(
        "{} did not start. Its messages are in {}",
        browser.display(),
        root.join("browser.log").display()
    )))
}

/// Has the browser killed when the thread that starts it ends, so a Patchwork that crashed or was
/// killed leaves no browser running. The thread is one of the app's long-lived workers.
#[cfg(target_os = "linux")]
pub(crate) fn die_with_patchwork(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: prctl is async-signal-safe and touches no memory of this process.
    unsafe {
        command.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        })
    };
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn die_with_patchwork(_: &mut Command) {}

/// Removes the profile folders under `root` whose browser is no longer running. A running browser
/// holds a `SingletonLock` link to `host-pid`.
#[cfg(target_os = "linux")]
fn remove_stale_profiles(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let folder = entry.path();
        if !entry.file_name().to_string_lossy().starts_with("profile-") {
            continue;
        }
        let running = std::fs::read_link(folder.join("SingletonLock"))
            .ok()
            .and_then(|target| {
                let target = target.to_string_lossy().into_owned();
                target.rsplit_once('-').map(|(_, pid)| pid.to_owned())
            })
            .is_some_and(|pid| Path::new("/proc").join(pid).exists());
        if !running && let Err(e) = std::fs::remove_dir_all(&folder) {
            log::warn!(error:% = e; "could not remove a browser profile: {{error}}");
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn remove_stale_profiles(_: &Path) {}

/// Waits up to `grace` for the browser to exit, kills it after that, and removes its profile.
pub(super) fn reap(mut child: Child, profile: &Path, grace: Duration) {
    let deadline = std::time::Instant::now() + grace;
    while std::time::Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    // Its helper processes may hold the folder a moment longer.
    std::thread::sleep(Duration::from_millis(200));
    if let Err(e) = std::fs::remove_dir_all(profile) {
        log::warn!(error:% = e; "could not remove a browser profile: {{error}}");
    }
}
