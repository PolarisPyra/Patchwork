//! The `Store`: everything is also kept in memory. Reads come from the cache, and every
//! change is written through to the database before it is visible. Writes for a thread only
//! touch its row and the messages that actually changed, since a streaming turn updates its
//! message on every event.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::migrations;
use super::legacy;
use super::settings::{load_settings, save_settings};
use super::threads::{load_folders, load_threads, save_message, save_thread_row, thread_state_keys};
use crate::projects::Folder;
use crate::settings::Settings;
use crate::system::instance;
use crate::threads::{MessageStatus, Thread};

#[derive(Debug)]
pub struct Store {
    inner: Mutex<Inner>,
    /// The lock on the data folder, held while this is the first Patchwork using it. A second
    /// one running beside it holds none.
    owner: Option<std::fs::File>,
    /// The threads whose goal was being pursued when the last session closed, and so was paused
    /// as this one opened.
    goals_cut_off: Vec<String>,
}

#[derive(Debug)]
pub(super) struct Inner {
    pub(super) conn: Connection,
    settings: Settings,
    threads: Vec<Thread>,
    folders: Vec<Folder>,
}

impl Store {
    /// Opens the database in `dir`, bringing its schema up to date and loading what it holds.
    ///
    /// # Errors
    ///
    /// Returns an error when the data folder or database cannot be created, opened, or migrated,
    /// or when stored settings, threads, or folders cannot be read or updated.
    pub fn open(dir: &Path) -> Result<Self, Error> {
        std::fs::create_dir_all(dir).map_err(err)?;
        let owner = claim(dir);
        let mut conn = open(dir)?;
        // A reply still streaming in the database belongs to an app that is gone: it was cut
        // off, and its turn can be resumed. Beside a running Patchwork the reply may be live in
        // that one, so the database is left alone and the reply is only shown as stopped here.
        if owner.is_some() {
            conn.execute(
                "UPDATE messages SET status = ?1 WHERE status = ?2",
                params![
                    enc(&MessageStatus::Interrupted),
                    enc(&MessageStatus::Streaming)
                ],
            )
            .map_err(err)?;
        }
        let settings = load_settings(&conn).map_err(err)?;
        crate::provider::set_agent_paths(&settings.agent_paths);
        crate::computer::set_enabled(settings.computer_use);
        let mut threads = load_threads(&conn).map_err(err)?;
        if owner.is_none() {
            threads
                .iter_mut()
                .flat_map(|t| &mut t.messages)
                .filter(|m| m.status == MessageStatus::Streaming)
                .for_each(|m| m.status = MessageStatus::Cancelled);
        }
        // A goal that was being pursued when the app quit is no longer pursued: its agent
        // process ended with the app. It is paused, so it can be resumed rather than shown as active.
        let tx = conn.transaction().map_err(err)?;
        let mut goals_cut_off = Vec::new();
        for t in &mut threads {
            if let Some(goal) = t
                .goal
                .as_mut()
                .filter(|g| g.status == crate::threads::GoalStatus::Active)
            {
                goal.status = crate::threads::GoalStatus::Paused;
                save_thread_row(&tx, t).map_err(err)?;
                goals_cut_off.push(t.id.clone());
            }
        }
        tx.commit().map_err(err)?;
        let folders = load_folders(&conn).map_err(err)?;
        Ok(Self {
            inner: Mutex::new(Inner {
                conn,
                settings,
                threads,
                folders,
            }),
            owner,
            goals_cut_off,
        })
    }

    /// The threads whose goal was being pursued when the last session closed. Each was paused as
    /// this one opened, and goes on when its turn is resumed.
    #[must_use]
    pub fn goals_cut_off(&self) -> &[String] {
        &self.goals_cut_off
    }

    /// Whether this is the first Patchwork using the data folder. Only that one resumes the
    /// turns an earlier run left unfinished, so two apps never run the same turn.
    #[must_use]
    pub fn owns_data(&self) -> bool {
        self.owner.is_some()
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    /// Changes the settings with `f` and saves them, returning what `f` returns.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot save the settings; the settings in memory stay as
    /// they were.
    pub fn update_settings<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> Result<R, Error> {
        let mut inner = self.lock();
        let mut next = inner.settings.clone();
        let r = f(&mut next);
        let tx = inner.conn.transaction().map_err(err)?;
        save_settings(&tx, &next).map_err(err)?;
        tx.commit().map_err(err)?;
        crate::provider::set_agent_paths(&next.agent_paths);
        crate::computer::set_enabled(next.computer_use);
        inner.settings = next;
        Ok(r)
    }

    pub fn threads(&self) -> Vec<Thread> {
        self.lock().threads.clone()
    }

    /// Runs `f` over the threads without copying them.
    pub fn with_threads<R>(&self, f: impl FnOnce(&[Thread]) -> R) -> R {
        f(&self.lock().threads)
    }

    pub fn thread(&self, id: &str) -> Option<Thread> {
        self.lock().threads.iter().find(|t| t.id == id).cloned()
    }

    /// `(agent, session_id)` of every thread that has a session, for de-duplicating imports.
    pub fn session_keys(&self) -> Vec<(crate::provider::AgentKind, String)> {
        self.lock()
            .threads
            .iter()
            .filter_map(|t| Some((t.agent, t.session_id.clone()?)))
            .collect()
    }

    /// OpenCode sessions already imported as threads.
    pub fn imported_sources(&self) -> Vec<String> {
        self.lock()
            .threads
            .iter()
            .filter_map(|t| t.imported_from.clone())
            .collect()
    }

    pub(crate) fn insert_threads(&self, threads: Vec<Thread>) -> Result<(), Error> {
        let mut inner = self.lock();
        let tx = inner.conn.transaction().map_err(err)?;
        for t in &threads {
            save_thread_row(&tx, t).map_err(err)?;
            for (i, m) in t.messages.iter().enumerate() {
                save_message(&tx, &t.id, i, m).map_err(err)?;
            }
        }
        tx.commit().map_err(err)?;
        inner.threads.extend(threads);
        Ok(())
    }

    /// Atomically remember the goal's dismissal and clear it from storage and memory.
    pub(crate) fn dismiss_goal(&self, id: &str, signature: &str) -> Result<(), Error> {
        let mut inner = self.lock();
        let index = inner
            .threads
            .iter()
            .position(|thread| thread.id == id)
            .ok_or(crate::threads::NOT_FOUND)?;
        let tx = inner.conn.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO app_state (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![format!("dismissed-goal:{id}"), signature],
        )
        .map_err(err)?;
        tx.execute("UPDATE threads SET goal = NULL WHERE id = ?1", [id])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        inner.threads[index].goal = None;
        Ok(())
    }

    /// Mutate one thread (no-op if missing) and persist what changed.
    pub(crate) fn update_thread(&self, id: &str, f: impl FnOnce(&mut Thread)) -> Result<(), Error> {
        let mut inner = self.lock();
        let Some(index) = inner.threads.iter().position(|t| t.id == id) else {
            return Ok(());
        };
        let mut next = inner.threads[index].clone();
        f(&mut next);
        let before = &inner.threads[index];
        let changed: Vec<usize> = (0..next.messages.len())
            .filter(|&i| before.messages.get(i) != Some(&next.messages[i]))
            .collect();
        let removed: Vec<String> = before
            .messages
            .iter()
            .filter(|old| !next.messages.iter().any(|message| message.id == old.id))
            .map(|message| message.id.clone())
            .collect();
        let tx = inner.conn.transaction().map_err(err)?;
        save_thread_row(&tx, &next).map_err(err)?;
        for message_id in removed {
            tx.execute(
                "DELETE FROM messages WHERE id = ?1 AND thread_id = ?2",
                params![message_id, next.id],
            )
            .map_err(err)?;
        }
        for i in changed {
            save_message(&tx, &next.id, i, &next.messages[i]).map_err(err)?;
        }
        tx.commit().map_err(err)?;
        inner.threads[index] = next;
        Ok(())
    }

    /// Mutates one thread in memory only (no-op if missing); the next save of it writes the change.
    pub(crate) fn update_unsaved(&self, id: &str, f: impl FnOnce(&mut Thread)) {
        if let Some(thread) = self.lock().threads.iter_mut().find(|t| t.id == id) {
            f(thread);
        }
    }

    /// Mutates one thread in place (no-op if missing) and saves its row and the message
    /// `message_id`, if given. Unlike `update_thread` it copies nothing, so it is for
    /// changes confined to that message and the thread's own fields; removing or
    /// reordering messages goes through `update_thread`.
    pub(crate) fn update_message(
        &self,
        id: &str,
        message_id: Option<&str>,
        f: impl FnOnce(&mut Thread),
    ) -> Result<(), Error> {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let Some(thread) = inner.threads.iter_mut().find(|t| t.id == id) else {
            return Ok(());
        };
        f(thread);
        let conn = &mut inner.conn;
        let written = (|| {
            let tx = conn.transaction()?;
            save_thread_row(&tx, thread)?;
            let message = message_id.and_then(|mid| {
                thread
                    .messages
                    .iter()
                    .enumerate()
                    .find(|(_, m)| m.id == mid)
            });
            if let Some((i, m)) = message {
                save_message(&tx, &thread.id, i, m)?;
            }
            tx.commit()
        })();
        if let Err(e) = written {
            // The thread goes back to what is stored, so memory never runs ahead of the disk.
            // Read back rather than copied first: this runs for every piece of a streamed reply.
            let stored = load_threads(conn)
                .ok()
                .and_then(|threads| threads.into_iter().find(|t| t.id == id));
            if let Some(stored) = stored {
                *thread = stored;
            }
            return Err(err(e).into());
        }
        Ok(())
    }

    /// A value from the `app_state` table, if one was stored.
    pub fn state(&self, key: &str) -> Option<String> {
        self.lock()
            .conn
            .query_row("SELECT value FROM app_state WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .ok()
            .flatten()
    }

    /// Stores `value` under `key` in the app state.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub fn set_state(&self, key: &str, value: &str) -> Result<(), Error> {
        self.lock()
            .conn
            .execute(
                "INSERT INTO app_state (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Forgets a stored value, so `state` answers None for it again.
    pub(crate) fn clear_state(&self, key: &str) -> Result<(), Error> {
        self.lock()
            .conn
            .execute("DELETE FROM app_state WHERE key = ?1", [key])
            .map_err(err)?;
        Ok(())
    }

    pub fn folders(&self) -> Vec<Folder> {
        self.lock().folders.clone()
    }

    /// Stores a new folder.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot insert the folder.
    pub fn create_folder(&self, folder: Folder) -> Result<(), Error> {
        let mut inner = self.lock();
        inner
            .conn
            .execute(
                "INSERT INTO folders (id, project_id, name, position)
                 VALUES (?1, ?2, ?3,
                         (SELECT COALESCE(MAX(position), -1) + 1 FROM folders WHERE project_id = ?2))",
                params![folder.id, folder.project_id, folder.name],
            )
            .map_err(err)?;
        inner.folders.push(folder);
        Ok(())
    }

    /// Renames the folder `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot update the folder.
    pub fn rename_folder(&self, id: &str, name: &str) -> Result<(), Error> {
        let mut inner = self.lock();
        inner
            .conn
            .execute(
                "UPDATE folders SET name = ?1 WHERE id = ?2",
                params![name, id],
            )
            .map_err(err)?;
        if let Some(folder) = inner.folders.iter_mut().find(|f| f.id == id) {
            folder.name = name.into();
        }
        Ok(())
    }

    /// Removes a folder. Its tasks move to the project's top level.
    pub(crate) fn delete_folder(&self, id: &str) -> Result<(), Error> {
        let mut inner = self.lock();
        let tx = inner.conn.transaction().map_err(err)?;
        // Back to the default order at the top level: newest first, as for a new task.
        tx.execute(
            "UPDATE threads SET folder_id = NULL, position = -created_at WHERE folder_id = ?1",
            [id],
        )
        .map_err(err)?;
        tx.execute("DELETE FROM folders WHERE id = ?1", [id])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        inner.folders.retain(|f| f.id != id);
        for t in &mut inner.threads {
            if t.folder_id.as_deref() == Some(id) {
                t.folder_id = None;
                t.position = -(t.created_at as i64);
            }
        }
        Ok(())
    }

    /// Puts `ids` in `folder_id` (None: the top level) in that order. The ids are the
    /// whole destination list, so a thread that came from elsewhere joins it here.
    /// Archived, pinned and favorite threads are not in `ids`; they keep their slots in the
    /// list, so restoring, unpinning or unfavoriting one puts it back where it was.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot save the new order.
    pub fn reorder_threads(&self, ids: &[String], folder_id: Option<&str>) -> Result<(), Error> {
        let mut inner = self.lock();
        // Listed elsewhere in the sidebar, so not among the rows that were dragged.
        let held = |t: &Thread| t.archived_at.is_some() || t.pinned || t.favorite;
        let ids = {
            let project_id = ids
                .first()
                .and_then(|id| inner.threads.iter().find(|t| t.id == *id))
                .map(|t| t.project_id.clone());
            let mut list: Vec<&Thread> = inner
                .threads
                .iter()
                .filter(|t| {
                    Some(&t.project_id) == project_id.as_ref()
                        && t.folder_id.as_deref() == folder_id
                        && (held(t) || ids.contains(&t.id))
                })
                .collect();
            list.sort_by_key(|t| t.position);
            let mut moved = ids.iter();
            let mut order: Vec<String> = list
                .iter()
                .filter_map(|t| {
                    if held(t) && !ids.contains(&t.id) {
                        Some(t.id.clone())
                    } else {
                        moved.next().cloned()
                    }
                })
                .collect();
            order.extend(moved.cloned());
            order
        };
        let tx = inner.conn.transaction().map_err(err)?;
        for (i, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE threads SET position = ?1, folder_id = ?2 WHERE id = ?3",
                params![i as i64, folder_id, id],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        for t in &mut inner.threads {
            if let Some(i) = ids.iter().position(|id| *id == t.id) {
                t.position = i as i64;
                t.folder_id = folder_id.map(String::from);
            }
        }
        Ok(())
    }

    /// Deletes a thread's messages and its agent session, so its next turn starts a new
    /// conversation. The thread itself stays.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot delete the chat's messages or reset its session
    /// and goal.
    pub fn clear_thread(&self, id: &str) -> Result<(), Error> {
        let mut inner = self.lock();
        let tx = inner.conn.transaction().map_err(err)?;
        tx.execute("DELETE FROM messages WHERE thread_id = ?1", [id])
            .map_err(err)?;
        tx.execute(
            "UPDATE threads SET session_id = NULL, goal = NULL WHERE id = ?1",
            [id],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        if let Some(t) = inner.threads.iter_mut().find(|t| t.id == id) {
            t.messages.clear();
            t.session_id = None;
            t.goal = None;
        }
        Ok(())
    }

    /// Deletes the chat and everything stored for it.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot delete the chat or its saved state.
    pub fn delete_thread(&self, id: &str) -> Result<(), Error> {
        let mut guard = self.lock();
        let inner = &mut *guard;
        // All of it or none: a thread row gone with its state left would come back empty.
        let tx = inner.conn.transaction().map_err(err)?;
        tx.execute("DELETE FROM threads WHERE id = ?1", [id])
            .map_err(err)?;
        for key in thread_state_keys(id) {
            tx.execute("DELETE FROM app_state WHERE key = ?1", [key])
                .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        inner.threads.retain(|t| t.id != id);
        Ok(())
    }

    /// Deletes a project's tasks and folders.
    pub(crate) fn delete_project_threads(&self, project_id: &str) -> Result<(), Error> {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let tx = inner.conn.transaction().map_err(err)?;
        tx.execute("DELETE FROM threads WHERE project_id = ?1", [project_id])
            .map_err(err)?;
        let ids: Vec<String> = inner
            .threads
            .iter()
            .filter(|t| t.project_id.as_deref() == Some(project_id))
            .map(|t| t.id.clone())
            .collect();
        for key in ids.iter().flat_map(|id| thread_state_keys(id)) {
            tx.execute("DELETE FROM app_state WHERE key = ?1", [key])
                .map_err(err)?;
        }
        tx.execute("DELETE FROM folders WHERE project_id = ?1", [project_id])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        inner
            .threads
            .retain(|t| t.project_id.as_deref() != Some(project_id));
        inner.folders.retain(|f| f.project_id != project_id);
        Ok(())
    }
}

const DB_FILE: &str = "patchwork.db";
/// Takes the data folder's lock, which the system releases when this process ends, however it
/// ends. None when another Patchwork holds it, or the lock file cannot be made.
fn claim(dir: &Path) -> Option<std::fs::File> {
    let file = std::fs::File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(instance::LOCK_FILE))
        .ok()?;
    file.try_lock().ok()?;
    instance::owns(dir);
    Some(file)
}

/// Opens the database in `dir`, creating it if need be, with the schema up to date. A new
/// database takes in the settings and threads of the old JSON files, if they are there.
fn open(dir: &Path) -> Result<Connection, Error> {
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

pub(super) fn err(e: impl std::fmt::Display) -> String {
    format!("Database error: {e}")
}

/// Enums are stored as their serde name, e.g. `"claude"`.
pub(super) fn enc<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        other => other.map(|v| v.to_string()).unwrap_or_default(),
    }
}

pub(super) fn dec<T: DeserializeOwned>(s: String) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

pub(super) fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".into())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "takes the column as read, so a row maps with `from_json(r.get(n)?)`"
)]
pub(super) fn from_json<T: DeserializeOwned + Default>(s: String) -> T {
    serde_json::from_str(&s).unwrap_or_default()
}
