//! The `settings` and `projects` tables.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::codec::{from_json, json};
use crate::types::Settings;

pub(super) fn save_settings(tx: &Transaction, settings: &Settings) -> rusqlite::Result<()> {
    // Projects live in their own table.
    let rest = Settings {
        projects: vec![],
        ..settings.clone()
    };
    tx.execute(
        "INSERT INTO settings (id, json) VALUES (1, ?1)
         ON CONFLICT (id) DO UPDATE SET json = excluded.json",
        [json(&rest)],
    )?;
    tx.execute("DELETE FROM projects", [])?;
    for (i, p) in settings.projects.iter().enumerate() {
        tx.execute(
            "INSERT INTO projects (id, name, path, position) VALUES (?1, ?2, ?3, ?4)",
            params![p.id, p.name, p.path, i],
        )?;
    }
    Ok(())
}


pub(super) fn load_settings(conn: &Connection) -> rusqlite::Result<Settings> {
    let mut settings: Settings = conn
        .query_row("SELECT json FROM settings WHERE id = 1", [], |r| r.get(0))
        .optional()?
        .map(from_json)
        .unwrap_or_default();
    let mut stmt = conn.prepare("SELECT id, name, path FROM projects ORDER BY position")?;
    settings.projects = stmt
        .query_map([], |r| {
            Ok(crate::types::Project {
                id: r.get(0)?,
                name: r.get(1)?,
                path: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(settings)
}

