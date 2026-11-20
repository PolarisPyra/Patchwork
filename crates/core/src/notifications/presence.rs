//! Whether the person is at Patchwork: its window's focus, as the desktop reports it.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use super::types::Place;
use crate::Core;
use crate::system::now_ms;

/// The window's focus, and the goals already told as met at each place.
#[derive(Debug, Default)]
pub(crate) struct Presence {
    focused: AtomicBool,
    /// When the window last lost focus, in Unix milliseconds; 0 until the desktop reports.
    since: AtomicU64,
    /// When a key or the mouse was last used in the window, in Unix milliseconds; 0 until the
    /// desktop reports.
    active: AtomicU64,
    /// The start of the goal each chat was last told as met, by place and thread id, so a goal
    /// reported again as met is told once.
    goals: Mutex<HashMap<(Place, String), u64>>,
}

impl Presence {
    /// Records that `started_at`'s goal of `thread_id` was met, and says whether it is the first
    /// time at `place`.
    pub(crate) fn first_goal(&self, place: Place, thread_id: &str, started_at: u64) -> bool {
        let mut goals = self.goals.locked();
        goals.insert((place, thread_id.to_string()), started_at) != Some(started_at)
    }
}

impl Core {
    /// Records whether Patchwork's window has focus. The desktop calls it each time that
    /// changes; while it has focus, nothing notifies.
    pub fn set_window_focused(&self, focused: bool) {
        let presence = &self.presence;
        if presence.focused.swap(focused, Ordering::SeqCst) != focused && !focused {
            presence.since.store(now_ms(), Ordering::SeqCst);
        }
    }

    /// Records that a key or the mouse was used in Patchwork's window just now.
    pub fn note_activity(&self) {
        self.presence.active.store(now_ms(), Ordering::Relaxed);
    }

    /// How long ago the window was last used, in milliseconds: 0 until the desktop reports.
    pub(crate) fn idle_ms(&self) -> u64 {
        match self.presence.active.load(Ordering::Relaxed) {
            0 => 0,
            at => now_ms().saturating_sub(at),
        }
    }

    /// Whether Patchwork's window has focus.
    pub(crate) fn window_focused(&self) -> bool {
        self.presence.focused.load(Ordering::SeqCst)
    }

    /// Whether the window has been out of focus for as long as the settings call away.
    pub(crate) fn away(&self) -> bool {
        if self.window_focused() {
            return false;
        }
        let after = u64::from(self.store.settings().notifications.away_after_secs) * 1000;
        now_ms().saturating_sub(self.presence.since.load(Ordering::SeqCst)) >= after
    }
}
