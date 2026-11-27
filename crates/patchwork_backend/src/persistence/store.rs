//! The `Store`: everything is also kept in memory. Reads come from the cache, and every
//! change is written through to the database before it is visible. Writes for a chat only
//! touch its row and the messages that actually changed, since a streaming turn updates its
//! message on every event.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use crate::Error;
use std::cell::OnceCell;
use std::collections::HashSet;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::migrations;
use super::settings::{load_settings, save_settings};
use super::chats::{
    load_folders, load_chats, load_outputs, save_message, save_chat_row, chat_state_keys,
};
use crate::projects::Folder;
use crate::settings::Settings;
use crate::system::processes::instance;
use crate::chats::{MessageStatus, Chat};

#[derive(Debug)]
pub struct Store {
    inner: Mutex<Inner>,
    /// The lock on the data folder, held while this is the first Patchwork using it. A second
    /// one running beside it holds none.
    owner: Option<std::fs::File>,
    /// The chats whose goal was being pursued when the last session closed, and so was paused
    /// as this one opened.
    goals_cut_off: Vec<String>,
}

#[derive(Debug)]
pub(super) struct Inner {
    pub(super) conn: Connection,
    pub(super) settings: Settings,
    pub(super) chats: Vec<Held>,
    pub(super) folders: Vec<Folder>,
}

/// A chat in memory beside its entry in the chat list, which is made once and dropped
/// whenever the chat is borrowed to be changed.
///
/// A chat read from the database holds no tool outputs until it is first opened or changed.
#[derive(Debug)]
pub struct Held {
    pub(super) chat: Chat,
    summary: OnceCell<Arc<Chat>>,
    /// Whether the chat's tool outputs are in memory.
    outputs: bool,
}

impl Held {
    fn new(chat: Chat) -> Self {
        Self {
            chat,
            summary: OnceCell::new(),
            outputs: true,
        }
    }

    /// A chat from `load_chats`, whose tool outputs are still in the database.
    fn stripped(chat: Chat) -> Self {
        Self {
            outputs: false,
            ..Self::new(chat)
        }
    }
}

impl Inner {
    /// Reads the tool outputs of the chat at `index` into memory, once.
    ///
    /// Done before a chat is handed out whole or changed, so no save writes it without them.
    fn hydrate(&mut self, index: usize) -> rusqlite::Result<()> {
        let held = &mut self.chats[index];
        if held.outputs {
            return Ok(());
        }
        load_outputs(&self.conn, held)?;
        held.outputs = true;
        Ok(())
    }

    /// The index of the chat `id`, with its tool outputs read in.
    fn hydrated(&mut self, id: &str) -> Option<Result<usize, String>> {
        let index = self.chats.iter().position(|t| t.id == id)?;
        Some(self.hydrate(index).map(|()| index).map_err(err))
    }
}

impl Deref for Held {
    type Target = Chat;

    fn deref(&self) -> &Chat {
        &self.chat
    }
}

impl DerefMut for Held {
    fn deref_mut(&mut self) -> &mut Chat {
        self.summary.take();
        &mut self.chat
    }
}

impl Store {
    /// Opens the database in `dir`, bringing its schema up to date and loading what it holds.
    ///
    /// # Errors
    ///
    /// Returns an error when the data folder or database cannot be created, opened, or migrated,
    /// or when stored settings, chats, or folders cannot be read or updated.
    pub fn open(dir: &Path) -> Result<Self, Error> {
        std::fs::create_dir_all(dir).map_err(err)?;
        // The database holds what was said in chats, so its folder is the user's alone.
        #[cfg(unix)]
        if let Err(e) =
            std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        {
            log::warn!(error:% = e; "could not make the data folder private: {{error}}");
        }
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
        let mut chats = load_chats(&conn).map_err(err)?;
        if owner.is_none() {
            chats
                .iter_mut()
                .flat_map(|t| &mut t.messages)
                .filter(|m| m.is_streaming())
                .for_each(|m| Arc::make_mut(m).status = MessageStatus::Cancelled);
        }
        // A goal that was being pursued when the app quit is no longer pursued: its agent
        // process ended with the app. It is paused, so it can be resumed rather than shown as active.
        // Beside a running Patchwork the goal is that one's live work, and stays as it is.
        let tx = conn.transaction().map_err(err)?;
        let mut goals_cut_off = Vec::new();
        for t in chats.iter_mut().filter(|_| owner.is_some()) {
            if let Some(goal) = t
                .goal
                .take_if(|g| g.status == crate::chats::GoalStatus::Active)
            {
                let paused = crate::chats::GoalStatus::Paused;
                t.goal = Some(goal.with_status(paused, crate::system::now_ms()));
                save_chat_row(&tx, t).map_err(err)?;
                goals_cut_off.push(t.id.clone());
            }
        }
        tx.commit().map_err(err)?;
        let folders = load_folders(&conn).map_err(err)?;
        Ok(Self {
            inner: Mutex::new(Inner {
                conn,
                settings,
                chats: chats.into_iter().map(Held::stripped).collect(),
                folders,
            }),
            owner,
            goals_cut_off,
        })
    }

    /// The chats whose goal was being pursued when the last session closed. Each was paused as
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
        self.inner.locked()
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    /// Changes the settings with `f` and saves what `stored` makes of them, returning what `f`
    /// returns. The settings in memory are the ones `f` left.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot save the settings; the settings in memory stay as
    /// they were.
    pub fn update_settings_as<R>(
        &self,
        f: impl FnOnce(&mut Settings) -> R,
        stored: impl FnOnce(&Settings) -> Settings,
    ) -> Result<R, Error> {
        let mut inner = self.lock();
        let mut next = inner.settings.clone();
        let r = f(&mut next);
        // Made before the transaction: `stored` may write the database through the vault.
        let kept = stored(&next);
        let tx = inner.conn.transaction().map_err(err)?;
        save_settings(&tx, &kept).map_err(err)?;
        tx.commit().map_err(err)?;
        inner.settings = next;
        Ok(r)
    }

    /// A copy of every chat. A chat not yet opened has no tool outputs.
    pub fn chats(&self) -> Vec<Chat> {
        self.lock().chats.iter().map(|t| t.chat.clone()).collect()
    }

    /// Every chat's entry in the chat list. `make` builds one the first time it is asked
    /// for, and it is kept until its chat changes.
    pub(crate) fn summaries(&self, make: impl Fn(&Chat) -> Chat) -> Vec<Arc<Chat>> {
        self.lock()
            .chats
            .iter()
            .map(|t| Arc::clone(t.summary.get_or_init(|| Arc::new(make(&t.chat)))))
            .collect()
    }

    /// Runs `f` over the chats without copying them. A chat not yet opened has no tool outputs.
    pub fn with_chats<R>(&self, f: impl FnOnce(&[Held]) -> R) -> R {
        f(&self.lock().chats)
    }

    /// A copy of the chat `id`, with its tool outputs.
    pub fn chat(&self, id: &str) -> Option<Chat> {
        let mut inner = self.lock();
        let index = match inner.hydrated(id)? {
            Ok(index) => index,
            Err(e) => {
                log::warn!(chat:% = id, error:% = e; "could not read the tool outputs of {{chat}}: {{error}}");
                inner.chats.iter().position(|t| t.id == id)?
            }
        };
        Some(inner.chats[index].chat.clone())
    }

    /// `(agent, session_id)` of every chat that has a session, for de-duplicating imports.
    pub fn session_keys(&self) -> Vec<(crate::provider::AgentKind, String)> {
        self.lock()
            .chats
            .iter()
            .filter_map(|t| Some((t.agent, t.session_id.clone()?)))
            .collect()
    }

    /// OpenCode sessions already imported as chats.
    pub fn imported_sources(&self) -> Vec<String> {
        self.lock()
            .chats
            .iter()
            .filter_map(|t| t.imported_from.clone())
            .collect()
    }

    pub(crate) fn insert_chats(&self, chats: Vec<Chat>) -> Result<(), Error> {
        let mut inner = self.lock();
        let tx = inner.conn.transaction().map_err(err)?;
        for t in &chats {
            save_chat_row(&tx, t).map_err(err)?;
            for (i, m) in t.messages.iter().enumerate() {
                save_message(&tx, &t.id, i, m).map_err(err)?;
            }
        }
        tx.commit().map_err(err)?;
        inner.chats.extend(chats.into_iter().map(Held::new));
        Ok(())
    }

    /// Atomically remember the goal's dismissal and clear it from storage and memory.
    pub(crate) fn dismiss_goal(&self, id: &str, signature: &str) -> Result<(), Error> {
        let mut inner = self.lock();
        let index = inner
            .chats
            .iter()
            .position(|chat| chat.id == id)
            .ok_or(crate::chats::NOT_FOUND)?;
        let tx = inner.conn.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO app_state (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![format!("dismissed-goal:{id}"), signature],
        )
        .map_err(err)?;
        tx.execute("UPDATE chats SET goal = NULL WHERE id = ?1", [id])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        inner.chats[index].goal = None;
        Ok(())
    }

    /// Mutate one chat (no-op if missing) and persist what changed.
    pub(crate) fn update_chat(&self, id: &str, f: impl FnOnce(&mut Chat)) -> Result<(), Error> {
        let mut inner = self.lock();
        let Some(index) = inner.hydrated(id).transpose()? else {
            return Ok(());
        };
        let mut next = inner.chats[index].chat.clone();
        f(&mut next);
        let before = &inner.chats[index];
        // A message `f` left alone is still the one allocation, which is told without reading it.
        let changed: Vec<usize> = (0..next.messages.len())
            .filter(|&i| {
                let now = &next.messages[i];
                before
                    .messages
                    .get(i)
                    .is_none_or(|was| !Arc::ptr_eq(was, now) && was != now)
            })
            .collect();
        let kept: HashSet<&str> = next.messages.iter().map(|m| m.id.as_str()).collect();
        let removed: Vec<String> = before
            .messages
            .iter()
            .filter(|old| !kept.contains(old.id.as_str()))
            .map(|message| message.id.clone())
            .collect();
        let tx = inner.conn.transaction().map_err(err)?;
        save_chat_row(&tx, &next).map_err(err)?;
        for message_id in removed {
            tx.execute(
                "DELETE FROM messages WHERE id = ?1 AND chat_id = ?2",
                params![message_id, next.id],
            )
            .map_err(err)?;
        }
        for i in changed {
            save_message(&tx, &next.id, i, &next.messages[i]).map_err(err)?;
        }
        tx.commit().map_err(err)?;
        inner.chats[index] = Held::new(next);
        Ok(())
    }

    /// Mutates one chat in memory only (no-op if missing); the next save of it writes the change.
    pub(crate) fn update_unsaved(&self, id: &str, f: impl FnOnce(&mut Chat)) {
        let mut inner = self.lock();
        let index = match inner.hydrated(id) {
            None => return,
            Some(Ok(index)) => index,
            Some(Err(e)) => {
                log::warn!(chat:% = id, error:% = e; "could not read the tool outputs of {{chat}}: {{error}}");
                return;
            }
        };
        f(&mut inner.chats[index]);
    }

    /// Mutates one chat in place (no-op if missing) and saves its row and the message
    /// `message_id`, if given. Unlike `update_chat` it copies nothing, so it is for
    /// changes confined to that message and the chat's own fields; removing or
    /// reordering messages goes through `update_chat`.
    pub(crate) fn update_message(
        &self,
        id: &str,
        message_id: Option<&str>,
        f: impl FnOnce(&mut Chat),
    ) -> Result<(), Error> {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let Some(index) = inner.hydrated(id).transpose()? else {
            return Ok(());
        };
        let chat = &mut inner.chats[index];
        f(chat);
        let conn = &mut inner.conn;
        let written = (|| {
            let tx = conn.transaction()?;
            save_chat_row(&tx, chat)?;
            let message = message_id
                .and_then(|mid| chat.messages.iter().enumerate().find(|(_, m)| m.id == mid));
            if let Some((i, m)) = message {
                save_message(&tx, &chat.id, i, m)?;
            }
            tx.commit()
        })();
        if let Err(e) = written {
            // The chat goes back to what is stored, so memory never runs ahead of the disk.
            // Read back rather than copied first: this runs for every piece of a streamed reply.
            let stored = load_chats(conn)
                .ok()
                .and_then(|chats| chats.into_iter().find(|t| t.id == id));
            if let Some(stored) = stored {
                *chat = Held::stripped(stored);
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
            .unwrap_or_else(|e| {
                log::warn!(key:% = key, error:% = e; "could not read the app state {{key}}: {{error}}");
                None
            })
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

    /// The keys in the app state that start with `prefix`.
    pub(crate) fn state_keys(&self, prefix: &str) -> Vec<String> {
        let store = self.lock();
        let keys = store
            .conn
            .prepare("SELECT key FROM app_state WHERE substr(key, 1, length(?1)) = ?1")
            .and_then(|mut query| {
                query
                    .query_map([prefix], |r| r.get(0))?
                    .collect::<Result<Vec<String>, _>>()
            });
        keys.unwrap_or_else(|e| {
            log::warn!(prefix:% = prefix, error:% = e; "could not list the app state under {{prefix}}: {{error}}");
            Vec::new()
        })
    }

    /// Forgets a stored value, so `state` answers None for it again.
    pub(crate) fn clear_state(&self, key: &str) -> Result<(), Error> {
        self.lock()
            .conn
            .execute("DELETE FROM app_state WHERE key = ?1", [key])
            .map_err(err)?;
        Ok(())
    }

    /// Deletes a chat's messages and its agent session, so its next turn starts a new
    /// conversation. The chat itself stays.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot delete the chat's messages or reset its session
    /// and goal.
    pub fn clear_chat(&self, id: &str) -> Result<(), Error> {
        let mut inner = self.lock();
        let tx = inner.conn.transaction().map_err(err)?;
        tx.execute("DELETE FROM messages WHERE chat_id = ?1", [id])
            .map_err(err)?;
        tx.execute(
            "UPDATE chats SET session_id = NULL, goal = NULL WHERE id = ?1",
            [id],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        if let Some(t) = inner.chats.iter_mut().find(|t| t.id == id) {
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
    pub fn delete_chat(&self, id: &str) -> Result<(), Error> {
        let mut guard = self.lock();
        let inner = &mut *guard;
        // All of it or none: a chat row gone with its state left would come back empty.
        let tx = inner.conn.transaction().map_err(err)?;
        tx.execute("DELETE FROM chats WHERE id = ?1", [id])
            .map_err(err)?;
        for key in chat_state_keys(id) {
            tx.execute("DELETE FROM app_state WHERE key = ?1", [key])
                .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        inner.chats.retain(|t| t.id != id);
        Ok(())
    }

    /// Deletes a project's tasks and folders.
    pub(crate) fn delete_project_chats(&self, project_id: &str) -> Result<(), Error> {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let tx = inner.conn.transaction().map_err(err)?;
        tx.execute("DELETE FROM chats WHERE project_id = ?1", [project_id])
            .map_err(err)?;
        let ids: Vec<String> = inner
            .chats
            .iter()
            .filter(|t| t.project_id.as_deref() == Some(project_id))
            .map(|t| t.id.clone())
            .collect();
        for key in ids.iter().flat_map(|id| chat_state_keys(id)) {
            tx.execute("DELETE FROM app_state WHERE key = ?1", [key])
                .map_err(err)?;
        }
        tx.execute("DELETE FROM folders WHERE project_id = ?1", [project_id])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        inner
            .chats
            .retain(|t| t.project_id.as_deref() != Some(project_id));
        inner.folders.retain(|f| f.project_id != project_id);
        Ok(())
    }
}

pub(crate) const DB_FILE: &str = "patchwork.db";

impl Store {
    /// Copies the database in the folder `from` to the folder `to`, as one snapshot even while
    /// another Patchwork has it open. The copy's scheduled tasks and chat apps are turned off, so
    /// they do not also run from it. Does nothing when `to` has a database or `from` has none; true
    /// when it copied.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be read or the copy written.
    pub fn copy(from: &Path, to: &Path) -> Result<bool, Error> {
        let (source, target) = (from.join(DB_FILE), to.join(DB_FILE));
        if target.exists() || !source.is_file() {
            return Ok(false);
        }
        std::fs::create_dir_all(to).map_err(err)?;
        Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .and_then(|conn| conn.execute("VACUUM INTO ?1", [target.to_string_lossy()]))
            .map_err(err)?;
        // The copy runs no scheduled tasks and answers no chat apps: the released app does both.
        Connection::open(&target)
            .and_then(|conn| {
                conn.execute("UPDATE scheduled_tasks SET enabled = 0", [])?;
                // The released app's secrets stay its own: the copy gets a vault of its own,
                // without the chat app tokens. A database from before the vault has no tables.
                let sealed: bool = conn.query_row(
                    "SELECT count(*) = 2 FROM sqlite_master \
                     WHERE type = 'table' AND name IN ('secrets', 'vault')",
                    [],
                    |r| r.get(0),
                )?;
                if sealed {
                    conn.execute_batch("DELETE FROM secrets; DELETE FROM vault;")?;
                    // A terminal variable's value was in the vault, so only the removals stay.
                    conn.execute(
                        "UPDATE settings SET json = json_set(json, '$.terminal.env', json((\
                         SELECT json_group_object(key, json('null')) \
                         FROM json_each(settings.json, '$.terminal.env') WHERE type = 'null'))) \
                         WHERE json_type(json, '$.terminal.env') = 'object'",
                        [],
                    )?;
                }
                conn.execute(
                    "UPDATE settings SET json = json_set(json, '$.gateway.platforms', json((\
                     SELECT json_group_object(key, json_set(value, '$.enabled', json('false'))) \
                     FROM json_each(settings.json, '$.gateway.platforms')))) \
                     WHERE json_type(json, '$.gateway.platforms') = 'object'",
                    [],
                )
            })
            .map_err(err)?;
        Ok(true)
    }
}

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

/// Opens the database in `dir`, creating it if need be, with the schema up to date.
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
    migrations::run(&mut conn)?;
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
