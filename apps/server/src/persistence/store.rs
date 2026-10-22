//! The `Store`: everything is also kept in memory. Reads come from the cache, and every
//! change is written through to the database before it is visible. Writes for a thread only
//! touch its row and the messages that actually changed, since a streaming turn updates its
//! message on every event.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::{Connection, OptionalExtension, params};

use super::codec::{Res, enc, err};
use super::connection;
use super::settings::{load_settings, save_settings};
use super::threads::{load_folders, load_threads, save_message, save_thread_row};
use crate::types::{Folder, MessageStatus, Settings, Thread};

pub struct Store {
    inner: Mutex<Inner>,
}

pub(super) struct Inner {
    pub(super) conn: Connection,
    settings: Settings,
    threads: Vec<Thread>,
    folders: Vec<Folder>,
}

impl Store {
    pub fn open(dir: PathBuf) -> Res<Self> {
        let mut conn = connection::open(&dir)?;
        // A turn that was streaming when the app quit can never finish.
        conn.execute(
            "UPDATE messages SET status = ?1 WHERE status = ?2",
            params![
                enc(&MessageStatus::Cancelled),
                enc(&MessageStatus::Streaming)
            ],
        )
        .map_err(err)?;
        let settings = load_settings(&conn).map_err(err)?;
        let mut threads = load_threads(&conn).map_err(err)?;
        // A goal that was being pursued when the app quit is no longer pursued: its agent
        // process ended with the app. It is paused, so it can be resumed rather than shown as active.
        let tx = conn.transaction().map_err(err)?;
        for t in threads.iter_mut() {
            if let Some(goal) = t.goal.as_mut().filter(|g| g.status == crate::types::GoalStatus::Active) {
                goal.status = crate::types::GoalStatus::Paused;
                save_thread_row(&tx, t).map_err(err)?;
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
        })
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    pub fn update_settings<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> Res<R> {
        let mut inner = self.lock();
        let mut next = inner.settings.clone();
        let r = f(&mut next);
        let tx = inner.conn.transaction().map_err(err)?;
        save_settings(&tx, &next).map_err(err)?;
        tx.commit().map_err(err)?;
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
    pub fn session_keys(&self) -> Vec<(crate::types::AgentKind, String)> {
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

    pub fn insert_threads(&self, threads: Vec<Thread>) -> Res<()> {
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
    pub fn dismiss_goal(&self, id: &str, signature: &str) -> Res<()> {
        let mut inner = self.lock();
        let index = inner
            .threads
            .iter()
            .position(|thread| thread.id == id)
            .ok_or("Thread not found")?;
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
    pub fn update_thread(&self, id: &str, f: impl FnOnce(&mut Thread)) -> Res<()> {
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

    /// Mutates one thread in place (no-op if missing) and saves its row and the message
    /// `message_id`, if given. Unlike `update_thread` it copies nothing, so it is for
    /// changes confined to that message and the thread's own fields; removing or
    /// reordering messages goes through `update_thread`.
    pub fn update_message(
        &self,
        id: &str,
        message_id: Option<&str>,
        f: impl FnOnce(&mut Thread),
    ) -> Res<()> {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let Some(thread) = inner.threads.iter_mut().find(|t| t.id == id) else {
            return Ok(());
        };
        f(thread);
        let tx = inner.conn.transaction().map_err(err)?;
        save_thread_row(&tx, thread).map_err(err)?;
        let message = message_id.and_then(|mid| {
            thread
                .messages
                .iter()
                .enumerate()
                .find(|(_, m)| m.id == mid)
        });
        if let Some((i, m)) = message {
            save_message(&tx, &thread.id, i, m).map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(())
    }

    /// A value from the `app_state` table, if one was stored.
    pub fn state(&self, key: &str) -> Option<String> {
        self.lock()
            .conn
            .query_row("SELECT value FROM app_state WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_state(&self, key: &str, value: &str) -> Res<()> {
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

    pub fn folders(&self) -> Vec<Folder> {
        self.lock().folders.clone()
    }

    pub fn create_folder(&self, folder: Folder) -> Res<()> {
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

    pub fn rename_folder(&self, id: &str, name: &str) -> Res<()> {
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
    pub fn delete_folder(&self, id: &str) -> Res<()> {
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
    pub fn reorder_threads(&self, ids: &[String], folder_id: Option<&str>) -> Res<()> {
        let mut inner = self.lock();
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
    pub fn clear_thread(&self, id: &str) -> Res<()> {
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

    pub fn delete_thread(&self, id: &str) -> Res<()> {
        let mut inner = self.lock();
        inner
            .conn
            .execute("DELETE FROM threads WHERE id = ?1", [id])
            .map_err(err)?;
        inner.threads.retain(|t| t.id != id);
        Ok(())
    }

    /// Deletes a project's tasks and folders.
    pub fn delete_project_threads(&self, project_id: &str) -> Res<()> {
        let mut inner = self.lock();
        inner
            .conn
            .execute("DELETE FROM threads WHERE project_id = ?1", [project_id])
            .map_err(err)?;
        inner
            .conn
            .execute("DELETE FROM folders WHERE project_id = ?1", [project_id])
            .map_err(err)?;
        inner
            .threads
            .retain(|t| t.project_id.as_deref() != Some(project_id));
        inner.folders.retain(|f| f.project_id != project_id);
        Ok(())
    }
}

