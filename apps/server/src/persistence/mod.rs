//! App state persisted in SQLite (`patchwork.db` in the app config dir).
//!
//! Everything is also kept in memory: reads come from the cache, and every
//! change is written through to the database before it is visible. Writes for
//! a thread only touch its row and the messages that actually changed, since
//! a streaming turn updates its message on every event.

mod migrations;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::types::{Folder, Message, MessageStatus, Settings, Thread};

const DB_FILE: &str = "patchwork.db";

pub struct Store {
    inner: Mutex<Inner>,
}

struct Inner {
    conn: Connection,
    settings: Settings,
    threads: Vec<Thread>,
    folders: Vec<Folder>,
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("Database error: {e}")
}

/// Enums are stored as their serde name, e.g. `"claude"`.
fn enc<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        other => other.map(|v| v.to_string()).unwrap_or_default(),
    }
}

fn dec<T: DeserializeOwned>(s: String) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".into())
}

fn from_json<T: DeserializeOwned + Default>(s: String) -> T {
    serde_json::from_str(&s).unwrap_or_default()
}

/// Moves the database and legacy JSON files from `old` into `new`, once:
/// only when `new` has no database yet. Earlier builds kept them in the
/// app-identifier config folder.
pub fn adopt_old_dir(old: &Path, new: &Path) -> Res<()> {
    if old == new || new.join(DB_FILE).exists() || !old.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(new).map_err(err)?;
    let wal = format!("{DB_FILE}-wal");
    let shm = format!("{DB_FILE}-shm");
    let names = [
        DB_FILE,
        &wal,
        &shm,
        "settings.json",
        "threads.json",
        "settings.json.bak",
        "threads.json.bak",
    ];
    for name in names {
        let from = old.join(name);
        if from.exists() {
            std::fs::rename(&from, new.join(name)).map_err(err)?;
        }
    }
    // Leave the old folder only if something else is still in it.
    let _ = std::fs::remove_dir(old);
    Ok(())
}

impl Store {
    pub fn open(dir: PathBuf) -> Res<Self> {
        std::fs::create_dir_all(&dir).map_err(err)?;
        let mut conn = Connection::open(dir.join(DB_FILE)).map_err(err)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .map_err(err)?;
        let from = migrations::run(&mut conn).map_err(err)?;
        if from == 0 {
            import_legacy_json(&mut conn, &dir)?;
        }
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

    fn lock(&self) -> MutexGuard<'_, Inner> {
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
        let tx = inner.conn.transaction().map_err(err)?;
        save_thread_row(&tx, &next).map_err(err)?;
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

fn save_settings(tx: &Transaction, settings: &Settings) -> rusqlite::Result<()> {
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

fn save_thread_row(tx: &Transaction, t: &Thread) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO threads
             (id, project_id, title, agent, model, session_id, created_at, access_mode, goal,
              position, folder_id, effort, archived_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT (id) DO UPDATE SET project_id = excluded.project_id,
             title = excluded.title, agent = excluded.agent, model = excluded.model,
             session_id = excluded.session_id, access_mode = excluded.access_mode,
             goal = excluded.goal, effort = excluded.effort, archived_at = excluded.archived_at",
        params![
            t.id,
            t.project_id,
            t.title,
            enc(&t.agent),
            t.model,
            t.session_id,
            t.created_at,
            enc(&t.access_mode),
            t.goal.as_ref().map(json),
            t.position,
            t.folder_id,
            t.effort,
            t.archived_at
        ],
    )?;
    Ok(())
}

fn save_message(
    tx: &Transaction,
    thread_id: &str,
    position: usize,
    m: &Message,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO messages
             (id, thread_id, position, role, text, created_at, status, tools, plan, files, error,
              approvals, attachments, steer, finished_at, usage, queued)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT (id) DO UPDATE SET position = excluded.position, text = excluded.text,
             status = excluded.status, tools = excluded.tools, plan = excluded.plan,
             files = excluded.files, error = excluded.error, approvals = excluded.approvals,
             attachments = excluded.attachments, finished_at = excluded.finished_at,
             usage = excluded.usage, steer = excluded.steer, queued = excluded.queued",
        params![
            m.id,
            thread_id,
            position,
            enc(&m.role),
            m.text,
            m.created_at,
            enc(&m.status),
            json(&m.tools),
            json(&m.plan),
            json(&m.files),
            m.error,
            json(&m.approvals),
            json(&m.attachments),
            m.steer,
            m.finished_at,
            m.usage.as_ref().map(json),
            m.queued
        ],
    )?;
    Ok(())
}

fn load_settings(conn: &Connection) -> rusqlite::Result<Settings> {
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

fn load_folders(conn: &Connection) -> rusqlite::Result<Vec<Folder>> {
    let mut stmt = conn.prepare("SELECT id, project_id, name FROM folders ORDER BY position")?;
    let folders = stmt
        .query_map([], |r| {
            Ok(Folder {
                id: r.get(0)?,
                project_id: r.get(1)?,
                name: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(folders)
}

fn load_threads(conn: &Connection) -> rusqlite::Result<Vec<Thread>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, title, agent, model, session_id, created_at, access_mode, goal,
                position, folder_id, effort, archived_at
         FROM threads ORDER BY position",
    )?;
    let mut threads: Vec<Thread> = stmt
        .query_map([], |r| {
            Ok(Thread {
                id: r.get(0)?,
                project_id: r.get(1)?,
                title: r.get(2)?,
                agent: dec(r.get(3)?)?,
                model: r.get(4)?,
                session_id: r.get(5)?,
                created_at: r.get(6)?,
                access_mode: dec(r.get(7)?)?,
                messages: vec![],
                goal: r
                    .get::<_, Option<String>>(8)?
                    .and_then(|g| serde_json::from_str(&g).ok()),
                position: r.get(9)?,
                folder_id: r.get(10)?,
                effort: r.get(11)?,
                archived_at: r.get(12)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut stmt = conn.prepare(
        "SELECT id, role, text, created_at, status, tools, plan, files, error, approvals,
                attachments, steer, finished_at, usage, queued
         FROM messages WHERE thread_id = ?1 ORDER BY position",
    )?;
    for t in &mut threads {
        t.messages = stmt
            .query_map([&t.id], |r| {
                Ok(Message {
                    id: r.get(0)?,
                    role: dec(r.get(1)?)?,
                    text: r.get(2)?,
                    created_at: r.get(3)?,
                    status: dec(r.get(4)?)?,
                    tools: from_json(r.get(5)?),
                    plan: from_json(r.get(6)?),
                    files: from_json(r.get(7)?),
                    error: r.get(8)?,
                    approvals: from_json(r.get(9)?),
                    attachments: from_json(r.get(10)?),
                    steer: r.get(11)?,
                    queued: r.get(14)?,
                    // Live only: the processes behind background commands die with the app.
                    background: Vec::new(),
                    finished_at: r.get(12)?,
                    usage: r
                        .get::<_, Option<String>>(13)?
                        .and_then(|u| serde_json::from_str(&u).ok()),
                    thinking: String::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
    }
    Ok(threads)
}

fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Moves `settings.json` / `threads.json` from before SQLite into a new database,
/// keeping the old files as `*.json.bak`.
fn import_legacy_json(conn: &mut Connection, dir: &Path) -> Res<()> {
    let settings_path = dir.join("settings.json");
    let threads_path = dir.join("threads.json");
    if !settings_path.exists() && !threads_path.exists() {
        return Ok(());
    }
    let mut settings: Settings = load_json(&settings_path);
    if settings.agents.is_empty() {
        settings.agents.extend(settings.agent.take());
    }
    let threads: Vec<Thread> = load_json(&threads_path);
    let tx = conn.transaction().map_err(err)?;
    save_settings(&tx, &settings).map_err(err)?;
    for t in &threads {
        save_thread_row(&tx, t).map_err(err)?;
        for (i, m) in t.messages.iter().enumerate() {
            save_message(&tx, &t.id, i, m).map_err(err)?;
        }
    }
    tx.commit().map_err(err)?;
    for path in [settings_path, threads_path] {
        if path.exists() {
            let _ = std::fs::rename(&path, path.with_extension("json.bak"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, Role, new_id};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(new_id());
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn message(text: &str, status: MessageStatus) -> Message {
        Message {
            id: new_id(),
            role: Role::Assistant,
            text: text.into(),
            created_at: 1,
            status,
            tools: vec![],
            plan: vec![],
            files: vec!["a.rs".into()],
            error: None,
            approvals: vec![],
            attachments: vec![],
            steer: false,
            queued: false,
            background: Vec::new(),
            thinking: String::new(),
            finished_at: None,
            usage: None,
        }
    }

    fn thread(messages: Vec<Message>) -> Thread {
        Thread {
            id: new_id(),
            project_id: None,
            title: "t".into(),
            agent: AgentKind::Antigravity,
            model: Some("pro".into()),
            session_id: Some("s".into()),
            access_mode: crate::types::AccessMode::Supervised,
            created_at: 2,
            messages,
            goal: None,
            position: 0,
            folder_id: None,
            effort: None,
            archived_at: None,
        }
    }

    #[test]
    fn old_dir_is_adopted_once() {
        let (old, new) = (temp_dir(), temp_dir().join("patchwork"));
        let store = Store::open(old.clone()).unwrap();
        store.update_settings(|s| s.onboarded = true).unwrap();
        drop(store);

        adopt_old_dir(&old, &new).unwrap();
        assert!(!old.exists());
        assert!(Store::open(new.clone()).unwrap().settings().onboarded);

        // A database already in the new folder wins.
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join(DB_FILE), "stale").unwrap();
        adopt_old_dir(&old, &new).unwrap();
        assert!(old.join(DB_FILE).exists());
        let _ = std::fs::remove_dir_all(&old);
        let _ = std::fs::remove_dir_all(new.parent().unwrap());
    }

    #[test]
    fn legacy_json_moves_into_sqlite() {
        let dir = temp_dir();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"onboarded":true,"agent":"codex","projects":[{"id":"p","name":"n","path":"/x"}]}"#,
        )
        .unwrap();
        let t = thread(vec![message("hi", MessageStatus::Streaming)]);
        std::fs::write(
            dir.join("threads.json"),
            serde_json::to_string(&[&t]).unwrap(),
        )
        .unwrap();

        let store = Store::open(dir.clone()).unwrap();
        let s = store.settings();
        assert_eq!(s.agents, [AgentKind::Codex]);
        assert_eq!(s.projects[0].path, "/x");
        let loaded = store.thread(&t.id).unwrap();
        assert_eq!(loaded.messages[0].status, MessageStatus::Cancelled);
        assert_eq!(loaded.messages[0].files, ["a.rs"]);
        assert!(dir.join("settings.json.bak").exists() && !dir.join("threads.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn thread_changes_survive_reopen_and_delete_cascades() {
        let dir = temp_dir();
        let store = Store::open(dir.clone()).unwrap();
        let t = thread(vec![message("a", MessageStatus::Complete)]);
        store.insert_threads(vec![t.clone()]).unwrap();
        store
            .update_thread(&t.id, |t| {
                t.title = "renamed".into();
                t.messages[0].text.push('b');
                t.messages.push(message("c", MessageStatus::Complete));
            })
            .unwrap();
        drop(store);

        let store = Store::open(dir.clone()).unwrap();
        let loaded = store.thread(&t.id).unwrap();
        assert_eq!(loaded.title, "renamed");
        let texts: Vec<_> = loaded.messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["ab", "c"]);
        assert_eq!(store.session_keys(), [(AgentKind::Antigravity, "s".to_string())]);

        store.delete_thread(&t.id).unwrap();
        let count: i64 = store
            .lock()
            .conn
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
