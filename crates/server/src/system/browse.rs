//! Folder picking: the machines to choose from and browsing their folders.
//!
//! Picking a project folder from inside the app: the machines to pick from (this computer, plus the
//! SSH hosts in `~/.ssh/config`) and walking a machine's folders a level at a time.

// Rust guideline compliant 2026-10-09

use crate::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use super::fs::{home_dir, resolve};

/// The folders seen on SSH hosts this session, by host and path, so one seen before shows at
/// once while it is read again.
static LISTINGS: Mutex<Option<HashMap<(String, String), DirListing>>> = Mutex::new(None);

/// The most listings kept before they are all dropped.
const MOST_LISTINGS: usize = 512;

/// A machine the Add Project dialog can open a folder on: this computer, or a host from
/// `~/.ssh/config`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    /// This computer's hostname, or the `Host` alias from the SSH config.
    pub name: String,
    /// The SSH config's `HostName` for the alias, when it gives one.
    pub hostname: Option<String>,
    pub local: bool,
}

/// One sub-folder in a browsed directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
}

/// A directory's folders, for the in-app folder browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    /// The directory being shown, resolved to an absolute path.
    pub path: String,
    /// Its parent, so the browser can offer a step up; absent at the filesystem root.
    pub parent: Option<String>,
    /// The sub-folders, sorted by name.
    pub entries: Vec<DirEntry>,
}

/// This computer followed by every usable `Host` in `~/.ssh/config`.
#[must_use]
pub fn machines() -> Vec<Machine> {
    let mut list = vec![Machine {
        name: hostname(),
        hostname: None,
        local: true,
    }];
    list.extend(super::ssh::hosts().into_iter().map(|h| Machine {
        name: h.name,
        hostname: h.hostname,
        local: false,
    }));
    list
}

/// This computer's hostname, falling back to a plain label when it cannot be read.
fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "This computer".to_string())
}

/// The sub-folders of `path` (this computer's home folder when none is given), sorted by name.
///
/// A typed path may start with `~` and a relative one is taken from the home folder. Only
/// directories are listed, since the browser picks a project folder; symlinks are followed so a
/// linked project folder still shows.
///
/// # Errors
///
/// Returns an error when the path cannot be opened, is not a folder, or cannot be read.
pub fn browse(path: Option<&str>) -> Result<DirListing, Error> {
    let home = home_dir().unwrap_or_else(|| std::path::PathBuf::from("/"));
    let dir = match path.map(str::trim) {
        Some("~") => home,
        Some(p) if !p.is_empty() => resolve(&home, p),
        _ => home,
    };
    let dir =
        super::fs::canonicalize(&dir).map_err(|e| format!("Cannot open {}: {e}", dir.display()))?;
    if !dir.is_dir() {
        return Err(format!("{} is not a folder", dir.display()).into());
    }
    let mut entries: Vec<DirEntry> = std::fs::read_dir(&dir)
        .map_err(|e| format!("Cannot read {}: {e}", dir.display()))?
        .filter_map(std::result::Result::ok)
        // The entry says what it is without another look at the disk, except for a link.
        .filter(|e| match e.file_type() {
            Ok(kind) if kind.is_dir() => true,
            Ok(kind) if kind.is_symlink() => e.path().is_dir(),
            _ => false,
        })
        .map(|e| DirEntry {
            name: e.file_name().to_string_lossy().into_owned(),
            path: e.path().to_string_lossy().into_owned(),
        })
        .collect();
    entries.sort_by_key(|e| e.name.to_lowercase());
    Ok(DirListing {
        path: dir.to_string_lossy().into_owned(),
        parent: dir.parent().map(|p| p.to_string_lossy().into_owned()),
        entries,
    })
}

/// The sub-folders of `path` on the SSH host `host` (its home folder when none is given), sorted
/// by name. Paths are the host's own, as [`browse`] gives this computer's.
///
/// A path may start with `~`, and a relative one is taken from the home folder there. Links to
/// folders are listed, as on this computer. Patchwork's helper there lists them, so the paths
/// use forward slashes on every host.
///
/// # Errors
///
/// Returns an error when the host cannot be reached, or the path there cannot be opened.
pub fn browse_host(host: &str, path: Option<&str>) -> Result<DirListing, Error> {
    let listing = crate::system::remote::op::browse(host, path)?;
    let mut guard = LISTINGS.lock().unwrap_or_else(PoisonError::into_inner);
    let seen = guard.get_or_insert_with(HashMap::new);
    if seen.len() >= MOST_LISTINGS {
        seen.clear();
    }
    // Under the path asked for and the one it resolved to, which a step down or up asks for.
    for key in [listing_key(path), listing_key(Some(&listing.path))] {
        seen.insert((host.to_string(), key), listing.clone());
    }
    Ok(listing)
}

/// What [`browse_host`] last found at `path` on `host`, to show while it is read again.
#[must_use]
pub fn remembered(host: &str, path: Option<&str>) -> Option<DirListing> {
    LISTINGS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()?
        .get(&(host.to_string(), listing_key(path)))
        .cloned()
}

/// `path` as [`LISTINGS`] names it: empty for the home folder, and without a closing slash.
fn listing_key(path: Option<&str>) -> String {
    let path = path.map_or("", str::trim);
    let cut = path.trim_end_matches(['/', '\\']);
    // A root, `/` or `C:/`, keeps its slash.
    if cut.is_empty() || cut.ends_with(':') {
        path.to_string()
    } else {
        cut.to_string()
    }
}
