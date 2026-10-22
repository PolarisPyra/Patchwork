//! Data from before the database existed, or from before it moved: the old config folder, and
//! the JSON files that preceded SQLite.

use std::path::Path;

use rusqlite::Connection;
use serde::de::DeserializeOwned;

use super::codec::{Res, err};
use super::connection::DB_FILE;
use super::settings::save_settings;
use super::threads::{save_message, save_thread_row};
use crate::types::{Settings, Thread};

/// Moves the database and legacy JSON files from `old` into `new`, once:
/// only when `new` has no database yet. Earlier builds kept them in the
/// app-identifier config folder.
pub fn adopt_old_dir(old: &Path, new: &Path) -> Res<()> {
    if old == new || new.join(DB_FILE).exists() || !old.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(new).map_err(err)?;
    let wal = format!("{DB_FILE}-wal");
    let shm = format!("{DB_FILE}-shm");
    let names = [
        DB_FILE,
        &wal,
        &shm,
        "settings.json",
        "threads.json",
        "settings.json.bak",
        "threads.json.bak",
    ];
    for name in names {
        let from = old.join(name);
        if from.exists() {
            std::fs::rename(&from, new.join(name)).map_err(err)?;
        }
    }
    // Leave the old folder only if something else is still in it.
    let _ = std::fs::remove_dir(old);
    Ok(())
}

fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}


/// Moves `settings.json` / `threads.json` from before SQLite into a new database,
/// keeping the old files as `*.json.bak`.
pub(super) fn import_legacy_json(conn: &mut Connection, dir: &Path) -> Res<()> {
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
    for path in [settings_path, threads_path] {
        if path.exists() {
            let _ = std::fs::rename(&path, path.with_extension("json.bak"));
        }
    }
    Ok(())
}

