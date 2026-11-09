//! Folder picking: the machines to choose from and local folder browsing.
//!
//! Picking a project folder from inside the app: the machines to pick from (this computer, plus the
//! SSH hosts in `~/.ssh/config`) and walking this computer's folders a level at a time. Remote hosts
//! are listed so they can be chosen later; nothing here reaches across SSH yet.

// Rust guideline compliant 2026-10-09

use crate::Error;
use serde::Serialize;

use super::fs::{home_dir, resolve};

/// A machine the Add Project dialog can open a folder on. `local` is this computer, the only one
/// that can be browsed today; the rest come from `~/.ssh/config` and are shown but not connectable.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    /// This computer's hostname, or the `Host` alias from the SSH config.
    pub name: String,
    /// The SSH config's `HostName` for the alias, when it gives one.
    pub hostname: Option<String>,
    pub local: bool,
}

/// One sub-folder in a browsed directory.
#[derive(Debug, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
}

/// A directory's folders, for the in-app folder browser.
#[derive(Debug, Serialize)]
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
    let dir = dir
        .canonicalize()
        .map_err(|e| format!("Cannot open {}: {e}", dir.display()))?;
    if !dir.is_dir() {
        return Err(format!("{} is not a folder", dir.display()).into());
    }
    let mut entries: Vec<DirEntry> = std::fs::read_dir(&dir)
        .map_err(|e| format!("Cannot read {}: {e}", dir.display()))?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
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
