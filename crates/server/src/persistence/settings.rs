//! The `settings` and `projects` tables.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::json;
use crate::settings::Settings;

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
    let stored: Option<String> = conn
        .query_row("SELECT json FROM settings WHERE id = 1", [], |r| r.get(0))
        .optional()?;
    let mut settings: Settings = match stored {
        None => Settings::default(),
        Some(text) => match serde_json::from_str(&text) {
            Ok(settings) => settings,
            // Kept apart, so saving the defaults cannot lose what the settings held.
            Err(e) => {
                log::warn!("the saved settings are not valid JSON ({e}); starting from defaults");
                conn.execute(
                    "INSERT INTO app_state (key, value) VALUES ('unreadable_settings', ?1)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    [&text],
                )?;
                Settings::default()
            }
        },
    };
    if settings.import_log_disabled {
        settings.logging.imports = crate::settings::LogLevel::Off;
    }
    if let Some(model) = settings.vision_model.take() {
        settings.vision.get_or_insert(crate::settings::VisionModel {
            agent: crate::provider::AgentKind::Api,
            model,
        });
    }
    crate::permissions::seed(&mut settings.permissions);
    // Claude compacted only near the model's full window by default, far past where it gets costly.
    let tokens = &mut settings.tokens;
    if !tokens.compact_window_defaulted {
        tokens.compact_window_defaulted = true;
        if tokens.claude_compact_window == 0 {
            tokens.claude_compact_window = 200_000;
        }
    }
    let mut stmt = conn.prepare("SELECT id, name, path FROM projects ORDER BY position")?;
    settings.projects = stmt
        .query_map([], |r| {
            Ok(crate::projects::Project {
                id: r.get(0)?,
                name: r.get(1)?,
                path: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(settings)
}
