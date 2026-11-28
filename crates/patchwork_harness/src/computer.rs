//! Computer use as Patchwork's own agent sees it: whether it is on, the actions it can ask for, and
//! the screen and pointer the app that runs the agent installs.
//!
//! The screenshot and the pointer and keyboard live in `patchwork_sidecars`, which this crate does
//! not link, so the program that has them hands them over with [`install`]. A helper without them,
//! such as the one on an SSH host, offers no computer tools.

// Rust guideline compliant 2026-11-27

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use patchwork_platform::Error;
use serde::Deserialize;

/// The screen and the pointer and keyboard that act for the agent.
pub trait Hands: Send + Sync {
    /// Does `action` with the pointer or keyboard.
    ///
    /// # Errors
    ///
    /// Returns an error when computer use is off or the action fails.
    fn perform(&self, action: &Action) -> Result<(), Error>;

    /// Saves a screenshot in `dir` and returns its path.
    ///
    /// # Errors
    ///
    /// Returns an error when the screen cannot be captured or saved.
    fn screenshot_to_file(&self, dir: &Path) -> Result<PathBuf, Error>;
}

/// The hands installed for this process.
static HANDS: OnceLock<Box<dyn Hands>> = OnceLock::new();

/// Gives the agent the screen and the pointer and keyboard; a second call is ignored.
pub fn install(hands: Box<dyn Hands>) {
    let _ = HANDS.set(hands);
}

/// What the agent is told when this process has no screen to act on.
const NO_HANDS: &str = "Computer use is not available here";

/// Does `action` with the pointer or keyboard.
///
/// # Errors
///
/// Returns an error when no hands are installed, computer use is off, or the action fails.
pub fn perform(action: &Action) -> Result<(), Error> {
    HANDS.get().ok_or(NO_HANDS)?.perform(action)
}

/// Saves a screenshot in `dir` and returns its path.
///
/// # Errors
///
/// Returns an error when no hands are installed or the screen cannot be captured or saved.
pub fn screenshot_to_file(dir: &Path) -> Result<PathBuf, Error> {
    HANDS.get().ok_or(NO_HANDS)?.screenshot_to_file(dir)
}

/// Whether this process has the screen and the pointer and keyboard to offer.
#[must_use]
pub fn available() -> bool {
    HANDS.get().is_some()
}

/// Whether Settings → Permissions has turned computer use on. Kept here so the tool list and the
/// approval rules can ask without a settings lookup.
static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Follows the setting; called whenever the settings are loaded or saved.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// Whether computer use is on.
pub fn enabled() -> bool {
    ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether Settings → Permissions lets the agent see the screen, apart from computer use. Kept
/// here for the same reason as [`ENABLED`].
static SEEING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Follows the setting that lets the agent see the screen; called with [`set_enabled`].
pub fn set_seeing(seeing: bool) {
    SEEING.store(seeing, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the agent may see the screen.
pub fn seeing() -> bool {
    SEEING.load(std::sync::atomic::Ordering::Relaxed)
}

fn left() -> String {
    "left".into()
}

/// What to do with the pointer or keyboard.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    Move {
        x: i32,
        y: i32,
    },
    Click {
        x: i32,
        y: i32,
        /// `left`, `middle`, or `right`.
        #[serde(default = "left")]
        button: String,
        #[serde(default)]
        double: bool,
    },
    /// Type text, as the keyboard would.
    Type {
        text: String,
    },
    /// Press a key, with modifiers held.
    Key {
        key: String,
        #[serde(default)]
        modifiers: Vec<String>,
    },
    /// Scroll by notches; positive is down, or right when `horizontal`.
    Scroll {
        amount: i32,
        #[serde(default)]
        horizontal: bool,
    },
}

/// A one-line description of an action, for the chat's tool rows and the log.
#[must_use]
pub fn describe(action: &Action) -> String {
    match action {
        Action::Move { x, y } => format!("Move the pointer to {x}, {y}"),
        Action::Click {
            x,
            y,
            button,
            double,
        } => {
            let times = if *double { "Double-click" } else { "Click" };
            format!("{times} {button} at {x}, {y}")
        }
        Action::Type { text } => {
            let mut shown: String = text.chars().take(40).collect();
            if text.chars().count() > 40 {
                shown.push('…');
            }
            format!("Type \"{shown}\"")
        }
        Action::Key { key, modifiers } => {
            let mut parts = modifiers.clone();
            parts.push(key.clone());
            format!("Press {}", parts.join("+"))
        }
        Action::Scroll { amount, horizontal } => {
            let axis = if *horizontal { "across" } else { "down" };
            format!("Scroll {amount} {axis}")
        }
    }
}
