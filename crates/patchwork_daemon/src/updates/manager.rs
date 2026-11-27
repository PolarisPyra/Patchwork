//! The `Updates` state machine and background update polling.

// Rust guideline compliant 2026-10-09

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use patchwork_platform::Locked;

use super::install;
use super::release;
use super::status::{ReleaseNotes, UpdateState, UpdateStatus};
use crate::restart::{self, RestartResult};

const FIRST_CHECK: Duration = Duration::from_secs(30);
const TICK: Duration = Duration::from_secs(60);

type Listener = Box<dyn Fn() + Send + Sync>;

/// Whether a check runs now, and who is looking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Check {
    #[default]
    Idle,
    /// On the timer. It changes what is shown only when it finds something, so going offline does
    /// not hide an update already found.
    Background,
    /// The user asked, so it shows its progress and its errors.
    Watched,
}

#[derive(Default)]
struct Inner {
    shown: UpdateState,
    check: Check,
    /// Counts the checks started, which tells a check from the one after it.
    run: u64,
    last_check: Option<Instant>,
    /// The notes of the newer release the last check found.
    notes: Option<ReleaseNotes>,
}

/// Where updating stands, and what to tell when it changes.
#[derive(Default)]
pub struct Updates {
    inner: Mutex<Inner>,
    listener: Mutex<Option<Listener>>,
}

impl fmt::Debug for Updates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Updates").finish_non_exhaustive()
    }
}

/// Ends a check that stopped without an outcome, as a panic would leave it. It holds the
/// check's number, so one that ended properly never ends the check started after it.
struct Running<'a>(&'a Updates, u64);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        let mut inner = self.0.lock();
        if inner.check == Check::Idle || inner.run != self.1 {
            return;
        }
        inner.check = Check::Idle;
        if matches!(
            inner.shown,
            UpdateState::Checking | UpdateState::Downloading { .. }
        ) {
            inner.shown = UpdateState::Failed {
                message: "The update check stopped.".into(),
            };
        }
        drop(inner);
        self.0.changed();
    }
}

impl Updates {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.locked()
    }

    /// Calls `listener` whenever the status changes, from the thread that changed it.
    pub fn set_listener(&self, listener: Listener) {
        *self.listener.locked() = Some(listener);
    }

    fn changed(&self) {
        let listener = self.listener.locked();
        if let Some(listener) = listener.as_ref() {
            listener();
        }
    }

    /// Where updating stands.
    #[must_use]
    pub fn status(&self) -> UpdateStatus {
        let inner = self.lock();
        UpdateStatus {
            can_install: install::can_install(),
            state: inner.shown.clone(),
            notes: inner.notes.clone(),
        }
    }

    /// Whether an update has been installed and is ready to run after a restart.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(self.lock().shown, UpdateState::Ready { .. })
    }

    /// Evaluates whether the app may restart into the installed update.
    #[must_use]
    pub fn restart_readiness(
        &self,
        running_turns: usize,
        force: bool,
        resumes_at_launch: bool,
    ) -> RestartResult {
        restart::evaluate(self.is_ready(), running_turns, force, resumes_at_launch)
    }

    fn due(&self, every: Duration) -> bool {
        self.lock()
            .last_check
            .is_none_or(|last| last.elapsed() >= every)
    }

    /// Takes the check, and returns whether the caller runs it: not while another runs, which a
    /// request from the user then watches, and not once an update is installed, since checking
    /// again would download it again.
    fn begin(&self, manual: bool) -> bool {
        let mut inner = self.lock();
        let started = match inner.check {
            Check::Background | Check::Watched => {
                if manual {
                    inner.check = Check::Watched;
                    if matches!(
                        inner.shown,
                        UpdateState::Idle | UpdateState::UpToDate | UpdateState::Failed { .. }
                    ) {
                        inner.shown = UpdateState::Checking;
                    }
                }
                false
            }
            Check::Idle => {
                inner.last_check = Some(Instant::now());
                let waiting = matches!(inner.shown, UpdateState::Ready { .. });
                if !waiting {
                    inner.run += 1;
                    inner.check = if manual {
                        inner.shown = UpdateState::Checking;
                        Check::Watched
                    } else {
                        Check::Background
                    };
                }
                !waiting
            }
        };
        drop(inner);
        if manual {
            self.changed();
        }
        started
    }

    /// Ends the running check. `outcome` is told whether the user watches it and what is shown,
    /// and returns what to show instead, if anything; a request that arrives now starts a new
    /// check rather than watching this one.
    fn finish(&self, outcome: impl FnOnce(bool, &UpdateState) -> Option<UpdateState>) {
        let mut inner = self.lock();
        let watched = inner.check == Check::Watched;
        if let Some(state) = outcome(watched, &inner.shown) {
            inner.shown = state;
        }
        inner.check = Check::Idle;
        drop(inner);
        self.changed();
    }

    /// Checks for a newer release, or an older one when the running version was withdrawn, then
    /// downloads and installs it. Blocks.
    pub fn check(&self, manual: bool) {
        if !self.begin(manual) {
            return;
        }
        let _running = Running(self, self.lock().run);
        let release = match release::latest() {
            Ok(release) => release,
            Err(e) => {
                log::warn!(error:% = e; "update check failed: {{error}}");
                let message = e.to_string();
                return self
                    .finish(|watched, _| watched.then_some(UpdateState::Failed { message }));
            }
        };
        if !release.replaces(patchwork_platform::VERSION) {
            return self.finish(|watched, shown| {
                let stale = matches!(shown, UpdateState::Idle | UpdateState::Failed { .. });
                (watched || stale).then_some(UpdateState::UpToDate)
            });
        }
        let version = release.version().to_string();
        self.lock().notes = release
            .body
            .as_deref()
            .map(|body| release::notes(&version, body))
            .filter(|notes| !notes.text.is_empty());
        if !install::can_install() {
            return self.finish(|_, _| Some(UpdateState::Available { version }));
        }
        let progress = |downloaded, total| {
            self.lock().shown = UpdateState::Downloading {
                version: version.clone(),
                downloaded,
                total,
            };
            self.changed();
        };
        let folder = patchwork_platform::dirs().cache.join("updates");
        let installed = release
            .download()
            .and_then(|download| download.fetch(&folder, &progress))
            .and_then(|file| install::install(&file));
        let state = match installed {
            Ok(()) => UpdateState::Ready { version },
            Err(e) => {
                log::warn!(version:% = version, error:% = e; "update {{version}} failed: {{error}}");
                UpdateState::Failed {
                    message: e.to_string(),
                }
            }
        };
        self.finish(|_, _| Some(state));
    }

    /// Runs a manual update check on its own thread.
    pub fn spawn_check(self: &Arc<Self>) {
        let updates = Arc::clone(self);
        let started = std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || updates.check(true));
        if let Err(e) = started {
            log::warn!(error:% = e; "could not start the update check: {{error}}");
        }
    }

    /// Starts the background checks, for the life of the app. A dev build runs its checkout's
    /// version, which is behind the latest release until it is pulled, so it never checks on its
    /// own; Settings can still check.
    pub fn start_background_checks(
        self: &Arc<Self>,
        check_every_minutes: impl Fn() -> u32 + Send + 'static,
    ) {
        #[cfg(windows)]
        install::sweep();
        if cfg!(debug_assertions) {
            return;
        }
        let updates = Arc::clone(self);
        let started = std::thread::Builder::new()
            .name("update-checks".into())
            .spawn(move || {
                std::thread::sleep(FIRST_CHECK);
                loop {
                    let minutes = check_every_minutes();
                    let every = Duration::from_secs(u64::from(minutes.clamp(10, 60)) * 60);
                    if updates.due(every) {
                        updates.check(false);
                    }
                    std::thread::sleep(TICK);
                }
            });
        if let Err(e) = started {
            log::warn!(error:% = e; "updates will not be checked: {{error}}");
        }
    }
}
