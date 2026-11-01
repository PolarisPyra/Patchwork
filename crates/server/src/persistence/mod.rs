//! App state persisted in SQLite (`patchwork.db` in the data folder): opening the file and
//! migrating it, and how values are written to columns as text, are here.
//!
//! - `store`: the `Store` the rest of the app uses, with its in-memory cache.
//! - `migrations`: the schema, as numbered SQL files, and the record of which have run.
//! - `settings`, `threads`: how each table's rows map to the app's types.
//! - `legacy`: data from before the database, or from before it moved.

mod legacy;
mod migrations;
mod scheduled;
mod settings;
mod store;
mod threads;

pub use legacy::adopt_old_dir;
pub use store::Store;

use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;
use serde::de::DeserializeOwned;

const DB_FILE: &str = "patchwork.db";

/// Opens the database in `dir`, creating it if need be, with the schema up to date. A new
/// database takes in the settings and threads of the old JSON files, if they are there.
fn open(dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(dir).map_err(err)?;
    let mut conn = Connection::open(dir.join(DB_FILE)).map_err(err)?;
    // With WAL, NORMAL syncs at checkpoints instead of every commit. A commit made a 4 ms fsync,
    // under the store's lock, for each tool event of a streaming reply. The database cannot be
    // corrupted this way; a power cut or OS crash can lose only the last commits.
    conn.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;",
    )
    .map_err(err)?;
    let from = migrations::run(&mut conn)?;
    if from == 0 {
        legacy::import_legacy_json(&mut conn, dir)?;
    }
    Ok(conn)
}

fn err(e: impl std::fmt::Display) -> String {
    format!("Database error: {e}")
}

/// Enums are stored as their serde name, e.g. `"claude"`.
fn enc<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        other => other.map(|v| v.to_string()).unwrap_or_default(),
    }
}

fn dec<T: DeserializeOwned>(s: String) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".into())
}

fn from_json<T: DeserializeOwned + Default>(s: String) -> T {
    serde_json::from_str(&s).unwrap_or_default()
}

#[cfg(test)]
mod tests;
