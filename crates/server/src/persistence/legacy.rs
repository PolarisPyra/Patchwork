//! Data from before the database existed, or from before it moved: the old config folder, and
//! the JSON files that preceded SQLite.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::Path;

use rusqlite::Connection;
use serde::de::DeserializeOwned;

use super::err;
use super::settings::save_settings;
use super::threads::{save_message, save_thread_row};
use crate::settings::Settings;
use crate::threads::Thread;

fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Moves `settings.json` / `threads.json` from before SQLite into a new database,
/// keeping the old files as `*.json.bak`.
pub(super) fn import_legacy_json(conn: &mut Connection, dir: &Path) -> Result<(), Error> {
    let settings_path = dir.join("settings.json");
    let threads_path = dir.join("threads.json");
    if !settings_path.exists() && !threads_path.exists() {
        return Ok(());
    }
    let mut settings: Settings = load_json(&settings_path);
    if settings.agents.is_empty() {
        settings.agents.extend(settings.agent.take());
    }
    let threads: Vec<Thread> = load_json(&threads_path);
    let tx = conn.transaction().map_err(err)?;
    save_settings(&tx, &settings).map_err(err)?;
    for t in &threads {
        save_thread_row(&tx, t).map_err(err)?;
        for (i, m) in t.messages.iter().enumerate() {
            save_message(&tx, &t.id, i, m).map_err(err)?;
        }
    }
    tx.commit().map_err(err)?;
    [settings_path, threads_path]
        .into_iter()
        .filter(|path| path.exists())
        .for_each(|path| {
            let _ = std::fs::rename(&path, path.with_extension("json.bak"));
        });
    Ok(())
}
