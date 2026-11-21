//! The `artifacts` and `artifact_versions` tables: what agents made for the person to look at,
//! and every version of it.

// Rust guideline compliant 2026-10-09

use rusqlite::{OptionalExtension, Row, params};

use super::store::{Store, err};
use crate::Error;
use crate::artifacts::{Artifact, ArtifactKind, ArtifactVersion};

/// The columns `artifact` reads, with the number of versions last.
const COLUMNS: &str = "a.id, a.thread_id, a.key, a.kind, a.title, a.language, a.created_at,
     a.updated_at, a.pinned, (SELECT COUNT(*) FROM artifact_versions v WHERE v.artifact_id = a.id)";

/// An artifact from a row of [`COLUMNS`].
fn artifact(r: &Row<'_>) -> rusqlite::Result<Artifact> {
    let kind: String = r.get(3)?;
    Ok(Artifact {
        id: r.get(0)?,
        thread_id: r.get(1)?,
        key: r.get(2)?,
        kind: ArtifactKind::parse(&kind).unwrap_or(ArtifactKind::Code),
        title: r.get(4)?,
        language: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
        pinned: r.get(8)?,
        versions: r.get(9)?,
    })
}

impl Store {
    /// Every artifact, the most recently changed first.
    pub(crate) fn artifacts(&self) -> Vec<Artifact> {
        let inner = self.lock();
        let rows = inner
            .conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM artifacts a ORDER BY a.updated_at DESC"
            ))
            .and_then(|mut stmt| {
                stmt.query_map([], artifact)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            });
        rows.unwrap_or_else(|e| {
            log::warn!(error:% = e; "could not read the artifacts: {{error}}");
            Vec::new()
        })
    }

    /// The artifact with `id`.
    pub(crate) fn artifact(&self, id: &str) -> Option<Artifact> {
        self.find_artifact("a.id = ?1", params![id])
    }

    /// The artifact the agent of `thread_id` calls `key`.
    pub(crate) fn artifact_by_key(&self, thread_id: &str, key: &str) -> Option<Artifact> {
        self.find_artifact("a.thread_id = ?1 AND a.key = ?2", params![thread_id, key])
    }

    fn find_artifact(&self, filter: &str, values: &[&dyn rusqlite::ToSql]) -> Option<Artifact> {
        let inner = self.lock();
        inner
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM artifacts a WHERE {filter}"),
                values,
                artifact,
            )
            .optional()
            .unwrap_or_else(|e| {
                log::warn!(error:% = e; "could not read an artifact: {{error}}");
                None
            })
    }

    /// Every version of the artifact `id`, the oldest first.
    pub(crate) fn artifact_versions(&self, id: &str) -> Vec<ArtifactVersion> {
        let inner = self.lock();
        let rows = inner
            .conn
            .prepare(
                "SELECT n, content, created_at FROM artifact_versions
                 WHERE artifact_id = ?1 ORDER BY n",
            )
            .and_then(|mut stmt| {
                stmt.query_map(params![id], |r| {
                    Ok(ArtifactVersion {
                        n: r.get(0)?,
                        content: r.get(1)?,
                        created_at: r.get(2)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
            });
        rows.unwrap_or_else(|e| {
            log::warn!(error:% = e; "could not read an artifact's versions: {{error}}");
            Vec::new()
        })
    }

    /// Saves `content` as the next version of `artifact`, adding the artifact when it is new and
    /// taking its title and language when it is not. Returns the version's number.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub(crate) fn save_artifact_version(
        &self,
        artifact: &Artifact,
        content: &str,
    ) -> Result<u32, Error> {
        let mut inner = self.lock();
        let tx = inner.conn.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO artifacts (id, thread_id, key, kind, title, language, created_at,
             updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (id) DO UPDATE SET kind = ?4, title = ?5, language = ?6, updated_at = ?8",
            params![
                artifact.id,
                artifact.thread_id,
                artifact.key,
                artifact.kind.name(),
                artifact.title,
                artifact.language,
                artifact.created_at,
                artifact.updated_at,
            ],
        )
        .map_err(err)?;
        let n: u32 = tx
            .query_row(
                "SELECT COALESCE(MAX(n), 0) + 1 FROM artifact_versions WHERE artifact_id = ?1",
                params![artifact.id],
                |r| r.get(0),
            )
            .map_err(err)?;
        tx.execute(
            "INSERT INTO artifact_versions (artifact_id, n, content, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![artifact.id, n, content, artifact.updated_at],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(n)
    }

    /// Pins the artifact `id`, or unpins it.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub(crate) fn set_artifact_pinned(&self, id: &str, pinned: bool) -> Result<(), Error> {
        self.lock()
            .conn
            .execute(
                "UPDATE artifacts SET pinned = ?2 WHERE id = ?1",
                params![id, pinned],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Gives the artifact `id` the title `title`.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub(crate) fn rename_artifact(&self, id: &str, title: &str) -> Result<(), Error> {
        self.lock()
            .conn
            .execute(
                "UPDATE artifacts SET title = ?2 WHERE id = ?1",
                params![id, title],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Removes the artifact `id` and every version of it.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub(crate) fn delete_artifact(&self, id: &str) -> Result<(), Error> {
        let mut inner = self.lock();
        let tx = inner.conn.transaction().map_err(err)?;
        tx.execute(
            "DELETE FROM artifact_versions WHERE artifact_id = ?1",
            params![id],
        )
        .map_err(err)?;
        tx.execute("DELETE FROM artifacts WHERE id = ?1", params![id])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(())
    }
}
