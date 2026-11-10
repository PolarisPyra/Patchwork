//! The `Updates` state, and the `Server` operations that check, install, and restart.

// Rust guideline compliant 2026-10-09

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use super::install;
use super::release;
use super::types::{RestartResult, UpdateState, UpdateStatus};
use crate::{Error, Server};

const FIRST_CHECK: Duration = Duration::from_secs(30);
const TICK: Duration = Duration::from_secs(60);
/// The saved version whose update card the user closed.
const DISMISSED_KEY: &str = "update-dismissed";

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
    last_check: Option<Instant>,
    /// The notes of the newer release the last check found.
    notes: Option<String>,
}

/// Where updating stands, and what to tell when it changes.
#[derive(Default)]
pub(crate) struct Updates {
    inner: Mutex<Inner>,
    listener: Mutex<Option<Listener>>,
}

/// Ends a check that stopped without an outcome, as a panic would leave it.
struct Running<'a>(&'a Updates);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        let mut inner = self.0.lock();
        if inner.check == Check::Idle {
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
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn changed(&self) {
        let listener = self.listener.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(listener) = listener.as_ref() {
            listener();
        }
    }

    fn status(&self) -> UpdateStatus {
        let inner = self.lock();
        UpdateStatus {
            can_install: install::can_install(),
            state: inner.shown.clone(),
            notes: inner.notes.clone(),
        }
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
    fn check(&self, manual: bool) {
        if !self.begin(manual) {
            return;
        }
        let _running = Running(self);
        let release = match release::latest() {
            Ok(release) => release,
            Err(e) => {
                log::warn!(error:% = e; "update check failed: {{error}}");
                let message = e.to_string();
                return self
                    .finish(|watched, _| watched.then_some(UpdateState::Failed { message }));
            }
        };
        if !release.replaces(crate::VERSION) {
            return self.finish(|watched, shown| {
                let stale = matches!(shown, UpdateState::Idle | UpdateState::Failed { .. });
                (watched || stale).then_some(UpdateState::UpToDate)
            });
        }
        let version = release.version().to_string();
        self.lock().notes = release.body.clone().filter(|body| !body.trim().is_empty());
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
        let folder = crate::system::dirs().cache.join("updates");
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
}

impl Server {
    /// Where updating stands.
    pub fn update_status(&self) -> UpdateStatus {
        self.updates.status()
    }

    /// Calls `listener` whenever `update_status` changes, from the thread that changed it.
    pub fn on_update_change(&self, listener: Box<dyn Fn() + Send + Sync>) {
        *self
            .updates
            .listener
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(listener);
    }

    /// Checks for an update because the user asked, on a thread of its own, and installs what it
    /// finds. `update_status` shows its progress and its errors.
    pub fn check_for_updates(self: &Arc<Self>) {
        let server = Arc::clone(self);
        let started = std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || server.updates.check(true));
        if let Err(e) = started {
            log::warn!(error:% = e; "could not start the update check: {{error}}");
        }
    }

    /// Whether the app may restart into the installed update: not while agents are replying,
    /// unless `force` is on.
    pub fn restart_to_update(&self, force: bool) -> RestartResult {
        if !matches!(self.updates.lock().shown, UpdateState::Ready { .. }) {
            return RestartResult::NothingReady;
        }
        match self.running_turns() {
            running if running > 0 && !force => RestartResult::Busy { running },
            _ => RestartResult::Restart,
        }
    }

    /// Starts the background checks, for the life of the app. A dev build runs its checkout's
    /// version, which is behind the latest release until it is pulled, so it never checks on its
    /// own; Settings can still check.
    pub fn start_update_checks(self: &Arc<Self>) {
        #[cfg(windows)]
        install::sweep();
        if cfg!(debug_assertions) {
            return;
        }
        let server = Arc::clone(self);
        let started = std::thread::Builder::new()
            .name("update-checks".into())
            .spawn(move || {
                std::thread::sleep(FIRST_CHECK);
                loop {
                    let minutes = server.store.settings().updates.check_every_minutes;
                    let every = Duration::from_secs(u64::from(minutes.clamp(10, 60)) * 60);
                    if server.updates.due(every) {
                        server.updates.check(false);
                    }
                    std::thread::sleep(TICK);
                }
            });
        if let Err(e) = started {
            log::warn!(error:% = e; "updates will not be checked: {{error}}");
        }
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
