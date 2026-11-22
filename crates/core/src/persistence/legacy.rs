//! Data from before the database existed, or from before it moved: the old config folder, and
//! the JSON files that preceded SQLite.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::Path;

use rusqlite::Connection;
use serde::de::DeserializeOwned;

use super::err;
use super::settings::save_settings;
use super::chats::{save_message, save_chat_row};
use crate::settings::Settings;
use crate::chats::Chat;

fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    let Ok(text) = std::fs::read_to_string(path) else {
        return T::default();
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        // The file stays beside the database as `.bak`, so nothing in it is lost.
        log::warn!(path:% = path.display(), error:% = e; "could not read {{path}}, so nothing is imported from it: {{error}}");
        T::default()
    })
}

/// Moves `settings.json` / `chats.json` from before SQLite into a new database,
/// keeping the old files as `*.json.bak`.
pub(super) fn import_legacy_json(conn: &mut Connection, dir: &Path) -> Result<(), Error> {
    let settings_path = dir.join("settings.json");
    let chats_path = dir.join("threads.json");
    if !settings_path.exists() && !chats_path.exists() {
        return Ok(());
    }
    let mut settings: Settings = load_json(&settings_path);
    if settings.agents.is_empty() {
        settings.agents.extend(settings.agent.take());
    }
    let chats: Vec<Chat> = load_json(&chats_path);
    let tx = conn.transaction().map_err(err)?;
    save_settings(&tx, &settings).map_err(err)?;
    for t in &chats {
        save_chat_row(&tx, t).map_err(err)?;
        for (i, m) in t.messages.iter().enumerate() {
            save_message(&tx, &t.id, i, m).map_err(err)?;
        }
    }
    tx.commit().map_err(err)?;
    [settings_path, chats_path]
        .into_iter()
        .filter(|path| path.exists())
        .for_each(|path| {
            let _ = std::fs::rename(&path, path.with_extension("json.bak"));
        });
    Ok(())
}
