//! Restart coordination and relaunch target resolution.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

/// Whether the app may restart into an installed update right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartResult {
    /// Agents are replying in this many tasks; restarting would stop them.
    Busy { running: usize },
    /// Nothing is installed and waiting.
    NothingReady,
    /// The app may restart now.
    Restart,
}

/// Evaluates whether the app may restart given whether an update is ready, how many turns are
/// currently running, whether the user forced the restart, and whether launch resumes interrupted
/// replies.
#[must_use]
pub fn evaluate(
    ready: bool,
    running_turns: usize,
    force: bool,
    resumes_at_launch: bool,
) -> RestartResult {
    if !ready {
        return RestartResult::NothingReady;
    }
    match running_turns {
        running if running > 0 && !force && !resumes_at_launch => RestartResult::Busy { running },
        _ => RestartResult::Restart,
    }
}

/// What to start after an update, where it is not the running executable: the AppImage, whose
/// running copy is a file inside its mount.
#[must_use]
pub fn restart_path() -> Option<PathBuf> {
    if cfg!(any(windows, target_os = "macos")) {
        return None;
    }
    std::env::var_os("APPIMAGE").map(PathBuf::from)
}
