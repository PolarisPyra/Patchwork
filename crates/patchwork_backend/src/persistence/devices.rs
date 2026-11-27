//! The `devices` and `pair_codes` tables: the phones and computers paired with this one, and the
//! codes a new one trades for its token. Only a hash of each token is kept.

// Rust guideline compliant 2026-10-09

use rusqlite::{OptionalExtension, Row, params};

use super::store::Store;
use super::{err, json};
use crate::Error;
use crate::server::{Device, PairedDevice};

/// What a pairing code grants: the scopes, and the name it was made with, when it was given one.
pub type CodeGrant = (Vec<String>, Option<String>);

/// The columns `device` reads, in its order.
const COLUMNS: &str = "id, name, platform, scopes, created_at, last_seen_at, token_hint";

/// How long a device's last-seen time may lag behind, in milliseconds, so a device making many
/// requests writes the database about once a minute rather than on every one.
const SEEN_EVERY_MS: i64 = 60_000;

fn device(row: &Row<'_>) -> rusqlite::Result<PairedDevice> {
    Ok(PairedDevice {
        device: Device {
            id: row.get(0)?,
            name: row.get(1)?,
            platform: row.get(2)?,
            // Scopes that cannot be read grant nothing, rather than everything.
            scopes: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
            created_at_ms: row.get(4)?,
            last_seen_ms: row.get(5)?,
        },
        token_hint: row.get(6)?,
    })
}

impl Store {
    /// The paired devices, newest first.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be read.
    pub fn paired_devices(&self) -> Result<Vec<PairedDevice>, Error> {
        let inner = self.lock();
        let mut stmt = inner
            .conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM devices ORDER BY created_at DESC, id"
            ))
            .map_err(err)?;
        let rows = stmt.query_map([], device).map_err(err)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| err(e).into())
    }

    /// The device whose token hashes to `token_hash`, if one does.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be read.
    pub fn device_by_token_hash(&self, token_hash: &str) -> Result<Option<PairedDevice>, Error> {
        let inner = self.lock();
        inner
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM devices WHERE token_hash = ?1"),
                [token_hash],
                device,
            )
            .optional()
            .map_err(|e| err(e).into())
    }

    /// The device with `id`, if it is still paired.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be read.
    pub fn paired_device(&self, id: &str) -> Result<Option<PairedDevice>, Error> {
        let inner = self.lock();
        inner
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM devices WHERE id = ?1"),
                [id],
                device,
            )
            .optional()
            .map_err(|e| err(e).into())
    }

    /// Saves a newly paired device with the hash of its token.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub fn add_device(&self, paired: &PairedDevice, token_hash: &str) -> Result<(), Error> {
        let d = &paired.device;
        self.lock()
            .conn
            .execute(
                "INSERT INTO devices
                     (id, name, platform, scopes, token_hash, token_hint, created_at, last_seen_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    d.id,
                    d.name,
                    d.platform,
                    json(&d.scopes),
                    token_hash,
                    paired.token_hint,
                    d.created_at_ms,
                    d.last_seen_ms,
                ],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Forgets a device, so its token stops working. Whether it was paired.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub fn remove_device(&self, id: &str) -> Result<bool, Error> {
        let removed = self
            .lock()
            .conn
            .execute("DELETE FROM devices WHERE id = ?1", [id])
            .map_err(err)?;
        Ok(removed > 0)
    }

    /// Renames a device or changes its scopes, returning it as it now is; None when no device
    /// has `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub fn update_device(
        &self,
        id: &str,
        name: Option<&str>,
        scopes: Option<&[String]>,
    ) -> Result<Option<PairedDevice>, Error> {
        {
            let inner = self.lock();
            if let Some(name) = name {
                inner
                    .conn
                    .execute(
                        "UPDATE devices SET name = ?1 WHERE id = ?2",
                        params![name, id],
                    )
                    .map_err(err)?;
            }
            if let Some(scopes) = scopes {
                inner
                    .conn
                    .execute(
                        "UPDATE devices SET scopes = ?1 WHERE id = ?2",
                        params![json(&scopes), id],
                    )
                    .map_err(err)?;
            }
        }
        self.paired_device(id)
    }

    /// Notes that a device connected at `now_ms`, unless it was noted less than a minute ago.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub fn touch_device(&self, id: &str, now_ms: i64) -> Result<(), Error> {
        self.lock()
            .conn
            .execute(
                "UPDATE devices SET last_seen_at = ?1
                 WHERE id = ?2 AND (last_seen_at IS NULL OR last_seen_at < ?1 - ?3)",
                params![now_ms, id, SEEN_EVERY_MS],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Saves a pairing code, dropping the ones that have expired by `now_ms`.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub fn add_pair_code(
        &self,
        code: &str,
        scopes: &[String],
        name: Option<&str>,
        expires_at_ms: i64,
        now_ms: i64,
    ) -> Result<(), Error> {
        let mut inner = self.lock();
        let tx = inner.conn.transaction().map_err(err)?;
        tx.execute("DELETE FROM pair_codes WHERE expires_at <= ?1", [now_ms])
            .map_err(err)?;
        tx.execute(
            "INSERT INTO pair_codes (code, scopes, name, expires_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (code) DO UPDATE
                 SET scopes = excluded.scopes, name = excluded.name, expires_at = excluded.expires_at",
            params![code, json(&scopes), name, expires_at_ms],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(())
    }

    /// Uses up a pairing code: the scopes and name it was made with, when it exists and has not
    /// expired by `now_ms`. A code is gone after this either way, so it works once.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be read or written.
    pub fn take_pair_code(&self, code: &str, now_ms: i64) -> Result<Option<CodeGrant>, Error> {
        let mut inner = self.lock();
        let tx = inner.conn.transaction().map_err(err)?;
        let found: Option<(String, Option<String>, i64)> = tx
            .query_row(
                "SELECT scopes, name, expires_at FROM pair_codes WHERE code = ?1",
                [code],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(err)?;
        tx.execute("DELETE FROM pair_codes WHERE code = ?1", [code])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(found
            .filter(|(_, _, expires_at)| *expires_at > now_ms)
            .map(|(scopes, name, _)| (serde_json::from_str(&scopes).unwrap_or_default(), name)))
    }
}
