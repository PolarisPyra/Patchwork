//! `android-activity`-backed implementation of the GPUI platform.

// Rust guideline compliant 2026-10-09

mod dispatcher;
mod display;
mod input;
mod platform;
mod queue;
mod window;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use android_activity::AndroidApp;

pub use platform::AndroidPlatform;

thread_local! {
    /// The activity this thread's `android_main` runs.
    ///
    /// Android can create a new activity in a process whose old activity was destroyed, and
    /// `android-activity` then calls `android_main` again on a new thread, so each thread keeps its
    /// own.
    static APP: RefCell<Option<AndroidApp>> = const { RefCell::new(None) };
}

/// Hands the activity to GPUI. Call it first in `android_main`, before building the application.
pub fn init(app: AndroidApp) {
    APP.with_borrow_mut(|slot| *slot = Some(app));
}

/// Returns the activity this thread's `android_main` gave to [`init`].
///
/// # Panics
///
/// Panics if [`init`] has not been called on this thread.
pub(crate) fn app() -> AndroidApp {
    APP.with_borrow(Clone::clone).expect(
        "patchwork_android_platform::init must be called from android_main before GPUI starts",
    )
}

/// Returns the Android platform for GPUI. `headless` is ignored: an activity always has a window.
#[must_use]
pub fn current_platform(_headless: bool) -> Rc<dyn gpui::Platform> {
    Rc::new(AndroidPlatform::new())
}

/// Holds one callback that may be replaced or removed while it is running.
pub(crate) struct CallbackSlot<T> {
    value: RefCell<Option<T>>,
    generation: Cell<u64>,
}

impl<T> Default for CallbackSlot<T> {
    fn default() -> Self {
        Self {
            value: RefCell::new(None),
            generation: Cell::new(0),
        }
    }
}

impl<T> CallbackSlot<T> {
    pub(crate) fn set(&self, value: T) {
        self.generation.set(self.generation.get().wrapping_add(1));
        drop(self.value.replace(Some(value)));
    }

    pub(crate) fn take(&self) -> Option<T> {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.value.borrow_mut().take()
    }

    pub(crate) fn with<R>(&self, callback: impl FnOnce(&mut T) -> R) -> Option<R> {
        let mut value = self.value.borrow_mut().take()?;
        let generation = self.generation.get();
        let result = callback(&mut value);
        // A callback that application code replaced or removed while it ran stays gone.
        if self.generation.get() == generation {
            drop(self.value.replace(Some(value)));
        }
        Some(result)
    }
}
