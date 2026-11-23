//! The hooks folder, and the bundles in it.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::settings::Settings;

use super::{Bundle, read_bundle};

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Remembers the config folder, where the default hooks folder is.
///
/// The folder was called `plugins` before, so one left from then is renamed to `hooks`.
pub fn init(config_dir: &Path) {
    let _ = CONFIG_DIR.set(config_dir.to_path_buf());
    let (old, new) = (config_dir.join("plugins"), config_dir.join("hooks"));
    if old.is_dir()
        && !new.exists()
        && let Err(e) = std::fs::rename(&old, &new)
    {
        log::warn!(error:% = e; "could not rename the plugins folder to hooks: {{error}}");
    }
}

fn default_dir() -> PathBuf {
    CONFIG_DIR.get().cloned().unwrap_or_default().join("hooks")
}

/// The hooks folder in use.
pub fn dir(settings: &Settings) -> PathBuf {
    match settings.hooks_dir.as_deref().map(str::trim) {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => default_dir(),
    }
}

/// Every bundle in the hooks folder, by name.
pub fn list(settings: &Settings) -> Vec<Bundle> {
    let root = dir(settings);
    let mut bundles: Vec<Bundle> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let path = entry.path();
            let id = entry.file_name().to_string_lossy().into_owned();
            let enabled = !settings.disabled_hooks.contains(&id);
            read_bundle(&path, enabled)
        })
        .collect();
    bundles.sort_by_key(|bundle| bundle.name.to_lowercase());
    bundles
}
