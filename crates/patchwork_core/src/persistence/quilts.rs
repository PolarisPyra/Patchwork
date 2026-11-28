//! The `quilts` and `quilt_versions` tables: what agents made for the person to look at,
//! and every version of it.

// Rust guideline compliant 2026-10-09

use rusqlite::{OptionalExtension, Row, params};

use super::store::{Store, err};
use crate::Error;
use crate::quilts::{Quilt, QuiltKind, QuiltVersion};

/// The columns `quilt` reads, with the number of versions last.
const COLUMNS: &str = "a.id, a.chat_id, a.key, a.kind, a.title, a.language, a.created_at,
     a.updated_at, a.pinned, (SELECT COUNT(*) FROM quilt_versions v WHERE v.quilt_id = a.id)";

/// A quilt from a row of [`COLUMNS`].
fn quilt(r: &Row<'_>) -> rusqlite::Result<Quilt> {
    let kind: String = r.get(3)?;
    Ok(Quilt {
        id: r.get(0)?,
        chat_id: r.get(1)?,
        key: r.get(2)?,
        kind: QuiltKind::parse(&kind).unwrap_or(QuiltKind::Code),
        title: r.get(4)?,
        language: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
        pinned: r.get(8)?,
        versions: r.get(9)?,
    })
}

impl Store {
    /// Every quilt, the most recently changed first.
    pub(crate) fn quilts(&self) -> Vec<Quilt> {
        let inner = self.lock();
        let rows = inner
            .conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM quilts a ORDER BY a.updated_at DESC"
            ))
            .and_then(|mut stmt| {
                stmt.query_map([], quilt)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            });
        rows.unwrap_or_else(|e| {
            log::warn!(error:% = e; "could not read the quilts: {{error}}");
            Vec::new()
        })
    }

    /// The quilt with `id`.
    pub(crate) fn quilt(&self, id: &str) -> Option<Quilt> {
        self.find_quilt("a.id = ?1", params![id])
    }

    /// The quilt the agent of `chat_id` calls `key`.
    pub(crate) fn quilt_by_key(&self, chat_id: &str, key: &str) -> Option<Quilt> {
        self.find_quilt("a.chat_id = ?1 AND a.key = ?2", params![chat_id, key])
    }

    fn find_quilt(&self, filter: &str, values: &[&dyn rusqlite::ToSql]) -> Option<Quilt> {
        let inner = self.lock();
        inner
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM quilts a WHERE {filter}"),
                values,
                quilt,
            )
            .optional()
            .unwrap_or_else(|e| {
                log::warn!(error:% = e; "could not read a quilt: {{error}}");
                None
            })
    }

    /// Every version of the quilt `id`, the oldest first.
    pub(crate) fn quilt_versions(&self, id: &str) -> Vec<QuiltVersion> {
        let inner = self.lock();
        let rows = inner
            .conn
            .prepare(
                "SELECT n, content, created_at FROM quilt_versions
                 WHERE quilt_id = ?1 ORDER BY n",
            )
            .and_then(|mut stmt| {
                stmt.query_map(params![id], |r| {
                    Ok(QuiltVersion {
                        n: r.get(0)?,
                        content: r.get(1)?,
                        created_at: r.get(2)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
            });
        rows.unwrap_or_else(|e| {
            log::warn!(error:% = e; "could not read a quilt's versions: {{error}}");
            Vec::new()
        })
    }

    /// Saves `content` as the next version of `quilt`, adding the quilt when it is new and
    /// taking its title and language when it is not. Returns the version's number.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub(crate) fn save_quilt_version(&self, quilt: &Quilt, content: &str) -> Result<u32, Error> {
        let mut inner = self.lock();
        let tx = inner.conn.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO quilts (id, chat_id, key, kind, title, language, created_at,
             updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (id) DO UPDATE SET kind = ?4, title = ?5, language = ?6, updated_at = ?8",
            params![
                quilt.id,
                quilt.chat_id,
                quilt.key,
                quilt.kind.name(),
                quilt.title,
                quilt.language,
                quilt.created_at,
                quilt.updated_at,
            ],
        )
        .map_err(err)?;
        let n: u32 = tx
            .query_row(
                "SELECT COALESCE(MAX(n), 0) + 1 FROM quilt_versions WHERE quilt_id = ?1",
                params![quilt.id],
                |r| r.get(0),
            )
            .map_err(err)?;
        tx.execute(
            "INSERT INTO quilt_versions (quilt_id, n, content, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![quilt.id, n, content, quilt.updated_at],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(n)
    }

    /// Pins the quilt `id`, or unpins it.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub(crate) fn set_quilt_pinned(&self, id: &str, pinned: bool) -> Result<(), Error> {
        self.lock()
            .conn
            .execute(
                "UPDATE quilts SET pinned = ?2 WHERE id = ?1",
                params![id, pinned],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Gives the quilt `id` the title `title`.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub(crate) fn rename_quilt(&self, id: &str, title: &str) -> Result<(), Error> {
        self.lock()
            .conn
            .execute(
                "UPDATE quilts SET title = ?2 WHERE id = ?1",
                params![id, title],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Removes the quilt `id` and every version of it.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub(crate) fn delete_quilt(&self, id: &str) -> Result<(), Error> {
        let mut inner = self.lock();
        let tx = inner.conn.transaction().map_err(err)?;
        tx.execute(
            "DELETE FROM quilt_versions WHERE quilt_id = ?1",
            params![id],
        )
        .map_err(err)?;
        tx.execute("DELETE FROM quilts WHERE id = ?1", params![id])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(())
    }
}
