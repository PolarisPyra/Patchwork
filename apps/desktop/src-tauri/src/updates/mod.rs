//! Keeps Patchwork up to date from the releases published at updates.patchworkeditor.com. Shortly
//! after launch and then as often as Settings asks, it checks the latest release, downloads this
//! platform's file, checks it against the release's SHA-256 digest and the release signing key's
//! signature, and installs it. The new version
//! runs from the next start; the update card and Settings offer a restart. A copy that was not
//! installed from a release (a dev or source build) cannot replace itself, so it only reports the
//! version.

mod install;
mod release;
mod schedule;
mod state;

pub use schedule::start;
pub use state::*;
