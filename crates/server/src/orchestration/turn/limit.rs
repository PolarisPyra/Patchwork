//! A turn the agent's usage limit ended: when that limit resets, and resuming the turn then.
//!
//! The reply of such a turn ends with an error while one of the agent's usage windows is used
//! up. The user can ask for the turn to resume once the window resets; the wait is saved, so it
//! outlasts a restart, and a reset that passed while the app was closed resumes at launch.

// Rust guideline compliant 2026-10-09

use std::sync::{Arc, PoisonError};
use std::time::Duration;

use crate::system::now_ms;
use crate::threads::{MessageStatus, Thread};
use crate::{Error, Server};

/// A window counts as used up from here. Providers report a limit that was hit as 100, and one
/// rounds a little under.
const USED_UP: f64 = 99.5;

/// How long after the reset the turn resumes. A provider lets the limit go a little after the
/// time it gives, and a turn resumed too early only fails on the same limit again.
const MARGIN_MS: u64 = 60_000;

/// How often the waits are checked. A reset is hours away, so half a minute late is unseen.
const CHECK_EVERY: Duration = Duration::from_secs(30);

/// One turn waiting for a limit to reset.
#[derive(Debug, Clone)]
pub struct LimitWait {
    /// When it resumes, in Unix epoch milliseconds.
    at: u64,
    /// The reply that failed. The wait is dropped once the thread has moved past it.
    message_id: String,
}

fn key(thread_id: &str) -> String {
    format!("resume-at:{thread_id}")
}

impl Server {
    /// When the usage limit that ended the thread's last reply resets, in Unix epoch
    /// milliseconds. None unless that reply failed while one of its agent's usage windows was
    /// used up and is still to reset. Reads only what is already known, so it is cheap to call
    /// while drawing.
    #[must_use]
    pub fn limit_reset(&self, thread: &Thread) -> Option<u64> {
        thread
            .resumable_reply()
            .filter(|m| m.status == MessageStatus::Error)?;
        let now = now_ms();
        self.usage
            .known(thread.agent)?
            .windows
            .iter()
            .filter(|w| w.used_percent >= USED_UP)
            .filter_map(|w| w.resets_at)
            .filter(|at| *at > now)
            .max()
    }

    /// When the thread's turn is set to resume, if it is waiting for a limit to reset.
    #[must_use]
    pub fn limit_resume_at(&self, thread_id: &str) -> Option<u64> {
        let waits = self.runner.limit_waits.lock();
        let waits = waits.unwrap_or_else(PoisonError::into_inner);
        waits.get(thread_id).map(|wait| wait.at)
    }

    /// Sets the thread's failed turn to resume once its agent's usage limit has reset, and
    /// returns when.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown, when its last reply did not fail on a usage
    /// limit that is still to reset, or when the wait cannot be saved.
    pub fn resume_at_limit_reset(&self, thread_id: &str) -> Result<u64, Error> {
        let thread = self.thread(thread_id)?;
        let reset = self
            .limit_reset(&thread)
            .ok_or("This reply did not stop on a usage limit that is still to reset")?;
        let message_id = thread
            .resumable_reply()
            .map(|m| m.id.clone())
            .ok_or("This reply cannot be resumed")?;
        let at = reset + MARGIN_MS;
        self.store
            .set_state(&key(thread_id), &format!("{at}\n{message_id}"))?;
        let mut waits = self.runner.limit_waits.lock();
        let waits = waits.as_mut().unwrap_or_else(|e| e.get_mut());
        waits.insert(thread_id.into(), LimitWait { at, message_id });
        Ok(at)
    }

    /// Stops the thread's turn from resuming when its limit resets.
    ///
    /// # Errors
    ///
    /// Returns an error when the saved wait cannot be removed.
    pub fn cancel_limit_resume(&self, thread_id: &str) -> Result<(), Error> {
        let mut waits = self.runner.limit_waits.lock();
        waits
            .as_mut()
            .unwrap_or_else(|e| e.get_mut())
            .remove(thread_id);
        self.store.clear_state(&key(thread_id))
    }

    /// Takes up the waits an earlier session saved, and resumes each turn as its limit resets,
    /// for the life of the app. A Patchwork running beside another waits on none, as it resumes
    /// none.
    pub fn start_limit_resumes(self: &Arc<Self>) {
        if !self.store.owns_data() {
            return;
        }
        let saved: Vec<(String, LimitWait)> = self
            .store
            .with_threads(|threads| {
                threads
                    .iter()
                    .filter(|t| t.resumable_reply().is_some())
                    .map(|t| t.id.clone())
                    .collect::<Vec<_>>()
            })
            .into_iter()
            .filter_map(|id| {
                let note = self.store.state(&key(&id))?;
                let (at, message_id) = note.split_once('\n')?;
                let wait = LimitWait {
                    at: at.parse().ok()?,
                    message_id: message_id.into(),
                };
                Some((id, wait))
            })
            .collect();
        self.runner
            .limit_waits
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(saved);
        let server = Arc::clone(self);
        let started = std::thread::Builder::new()
            .name("limit-resume".into())
            .spawn(move || {
                loop {
                    server.resume_due();
                    std::thread::sleep(CHECK_EVERY);
                }
            });
        if let Err(e) = started {
            log::warn!(error:% = e; "could not start waiting on usage limits: {{error}}");
        }
    }

    /// Resumes the turns whose wait is over. A wait is used once, whether or not its turn
    /// starts: a thread that has moved on is left alone, and a turn that fails again on the
    /// limit can be set to wait again.
    fn resume_due(self: &Arc<Self>) {
        let now = now_ms();
        let due: Vec<(String, LimitWait)> = {
            let mut waits = self.runner.limit_waits.lock();
            let waits = waits.as_mut().unwrap_or_else(|e| e.get_mut());
            let ids: Vec<String> = waits
                .iter()
                .filter(|(_, wait)| wait.at <= now)
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| waits.remove(&id).map(|wait| (id, wait)))
                .collect()
        };
        for (thread_id, wait) in due {
            if let Err(e) = self.store.clear_state(&key(&thread_id)) {
                log::warn!(error:% = e; "could not drop a finished limit wait: {{error}}");
            }
            let still_waiting = self
                .store
                .thread(&thread_id)
                .is_some_and(|t| t.resumable_reply().is_some_and(|m| m.id == wait.message_id));
            if !still_waiting {
                continue;
            }
            if let Err(e) = self.resume_turn(&thread_id) {
                log::warn!(
                    thread = thread_id.as_str(), error:% = e;
                    "could not resume {{thread}} after its usage limit reset: {{error}}"
                );
            }
        }
    }
}
