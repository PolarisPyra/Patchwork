//! Opening the database file: its connection settings, its schema, and the move from the JSON
//! files that preceded it.

use std::path::Path;

use rusqlite::Connection;

use super::codec::{Res, err};
use super::{legacy, migrations};

pub(super) const DB_FILE: &str = "patchwork.db";

/// Opens the database in `dir`, creating it if need be, with the schema up to date. A new
/// database takes in the settings and threads of the old JSON files, if they are there.
pub(super) fn open(dir: &Path) -> Res<Connection> {
    std::fs::create_dir_all(dir).map_err(err)?;
    let mut conn = Connection::open(dir.join(DB_FILE)).map_err(err)?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
        .map_err(err)?;
    let from = migrations::run(&mut conn)?;
    if from == 0 {
        legacy::import_legacy_json(&mut conn, dir)?;
    }
    Ok(conn)
}
