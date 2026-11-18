//! The `bots` table: each bot's name, instructions, agent and linked projects.

// Rust guideline compliant 2026-10-09

use rusqlite::params;

use super::store::{Inner, Store};
use super::{dec, enc, err, json};
use crate::Error;
use crate::bots::{BlockedTools, Bot};
use crate::permissions::PermissionKind;

fn load(inner: &Inner) -> Result<Vec<Bot>, Error> {
    let mut stmt = inner
        .conn
        .prepare(
            "SELECT id, name, instructions, agent, model, effort, access_mode, projects, created_at,
                    archived_at, background, blocked
             FROM bots ORDER BY created_at",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Bot {
                id: row.get(0)?,
                name: row.get(1)?,
                instructions: row.get(2)?,
                agent: dec(row.get::<_, String>(3)?)?,
                model: row.get(4)?,
                effort: row.get(5)?,
                access_mode: dec(row.get::<_, String>(6)?)?,
                // A list that cannot be read links no project, rather than every one.
                projects: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                created_at: row.get(8)?,
                archived_at: row.get(9)?,
                // A background that cannot be read shows the shared one.
                background: row
                    .get::<_, Option<String>>(10)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                // A list that cannot be read blocks every kind of call and no server, rather
                // than nothing.
                blocked: match row.get::<_, Option<String>>(11)? {
                    None => BlockedTools::default(),
                    Some(s) => serde_json::from_str(&s).unwrap_or_else(|_| BlockedTools {
                        kinds: vec![
                            PermissionKind::Command,
                            PermissionKind::Edit,
                            PermissionKind::Web,
                        ],
                        servers: Vec::new(),
                    }),
                },
            })
        })
        .map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| err(e).into())
}

impl Store {
    /// Every bot, oldest first. A table that cannot be read gives none, and the log says why.
    pub fn bots(&self) -> Vec<Bot> {
        let inner = self.lock();
        load(&inner).unwrap_or_else(|e| {
            log::error!(error:% = e; "bots could not be read: {{error}}");
            Vec::new()
        })
    }

    /// Adds a bot, or replaces the one with its id.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub fn save_bot(&self, bot: &Bot) -> Result<(), Error> {
        let inner = self.lock();
        inner
            .conn
            .execute(
                "INSERT INTO bots
                     (id, name, instructions, agent, model, effort, access_mode, projects,
                      created_at, archived_at, background, blocked)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT (id) DO UPDATE SET name = excluded.name,
                     instructions = excluded.instructions, agent = excluded.agent,
                     model = excluded.model, effort = excluded.effort,
                     access_mode = excluded.access_mode, projects = excluded.projects,
                     archived_at = excluded.archived_at, background = excluded.background,
                     blocked = excluded.blocked",
                params![
                    bot.id,
                    bot.name,
                    bot.instructions,
                    enc(&bot.agent),
                    bot.model,
                    bot.effort,
                    enc(&bot.access_mode),
                    json(&bot.projects),
                    bot.created_at,
                    bot.archived_at,
                    bot.background.as_ref().map(json),
                    json(&bot.blocked),
                ],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Deletes bot `id`. Its runs stay, as plain chats.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot delete the bot.
    pub fn remove_bot(&self, id: &str) -> Result<(), Error> {
        let inner = self.lock();
        inner
            .conn
            .execute("DELETE FROM bots WHERE id = ?1", [id])
            .map_err(err)?;
        Ok(())
    }
}
