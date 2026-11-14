//! Keeps Patchwork up to date from its GitHub releases, each of which carries a latest.json.
//!
//! Shortly after launch and then as often as Settings asks, it reads the latest release,
//! downloads this system's file, checks it against the release's SHA-256 digest and the release
//! signing key's signature, and installs it. The new version runs from the next start; the update
//! card and Settings offer a restart. A copy that was not installed from a release (a dev or
//! source build) cannot replace itself, so it only reports the version.

mod install;
mod check;
mod release;
mod types;

pub use install::restart_path;
pub(crate) use check::Updates;
pub(crate) use release::helper as release_helper;
pub use release::{RELEASES_URL, notes};
pub use types::{ReleaseNotes, RestartResult, UpdateState, UpdateStatus};
