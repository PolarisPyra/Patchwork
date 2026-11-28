//! Keeps Patchwork up to date from its GitHub releases, each of which carries a latest.json.
//!
//! Shortly after launch and then as often as Settings asks, it reads the latest release,
//! downloads this system's file, checks it against the release's SHA-256 digest and the release
//! signing key's signature, and installs it. The new version runs from the next start; the update
//! card and Settings offer a restart. A copy that was not installed from a release (a dev or
//! source build) cannot replace itself, so it only reports the version.

// Rust guideline compliant 2026-10-09

mod install;
mod manager;
mod release;
mod status;

pub use crate::restart::{RestartResult, restart_path};
pub use manager::Updates;
pub use release::{RELEASES_URL, helper as release_helper, notes};
pub use status::{ReleaseNotes, UpdateSettings, UpdateState, UpdateStatus};
