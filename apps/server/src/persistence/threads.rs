//! The `threads`, `messages` and `folders` tables.

use rusqlite::{Connection, Transaction, params};

use super::codec::{dec, enc, from_json, json};
use crate::types::{Folder, Message, Thread};

pub(super) fn save_thread_row(tx: &Transaction, t: &Thread) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO threads
             (id, project_id, title, agent, model, session_id, created_at, access_mode, goal,
              position, folder_id, effort, archived_at, parent, plan_mode, slash_commands,
              imported_from, context_pending)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
         ON CONFLICT (id) DO UPDATE SET project_id = excluded.project_id,
             title = excluded.title, agent = excluded.agent, model = excluded.model,
             session_id = excluded.session_id, access_mode = excluded.access_mode,
             goal = excluded.goal, effort = excluded.effort, archived_at = excluded.archived_at,
             parent = excluded.parent, plan_mode = excluded.plan_mode,
             slash_commands = excluded.slash_commands, imported_from = excluded.imported_from,
             context_pending = excluded.context_pending",
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
            t.archived_at,
            t.parent.as_ref().map(json),
            t.plan_mode,
            json(&t.slash_commands),
            t.imported_from,
            t.context_pending
        ],
    )?;
    Ok(())
}

pub(super) fn save_message(
    tx: &Transaction,
    thread_id: &str,
    position: usize,
    m: &Message,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO messages
             (id, thread_id, position, role, text, created_at, status, tools, plan, files, error,
              approvals, attachments, steer, finished_at, usage, queued, parts, cut_off, refusal)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                 ?19, ?20)
         ON CONFLICT (id) DO UPDATE SET position = excluded.position, text = excluded.text,
             status = excluded.status, tools = excluded.tools, plan = excluded.plan,
             files = excluded.files, error = excluded.error, approvals = excluded.approvals,
             attachments = excluded.attachments, finished_at = excluded.finished_at,
             usage = excluded.usage, steer = excluded.steer, queued = excluded.queued,
             parts = excluded.parts, cut_off = excluded.cut_off, refusal = excluded.refusal",
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
            m.queued,
            json(&m.parts),
            m.cut_off,
            m.refusal.as_ref().map(json)
        ],
    )?;
    Ok(())
}


pub(super) fn load_folders(conn: &Connection) -> rusqlite::Result<Vec<Folder>> {
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


pub(super) fn load_threads(conn: &Connection) -> rusqlite::Result<Vec<Thread>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, title, agent, model, session_id, created_at, access_mode, goal,
                position, folder_id, effort, archived_at, parent, plan_mode, slash_commands,
                imported_from, context_pending
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
                parent: r
                    .get::<_, Option<String>>(13)?
                    .and_then(|p| serde_json::from_str(&p).ok()),
                plan_mode: r.get(14)?,
                slash_commands: from_json(r.get(15)?),
                imported_from: r.get(16)?,
                context_pending: r.get(17)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut stmt = conn.prepare(
        "SELECT id, role, text, created_at, status, tools, plan, files, error, approvals,
                attachments, steer, finished_at, usage, queued, parts, cut_off, refusal
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
                    cut_off: r.get(16)?,
                    refusal: r
                        .get::<_, Option<String>>(17)?
                        .and_then(|j| serde_json::from_str(&j).ok()),
                    parts: from_json(r.get(15)?),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
    }
    Ok(threads)
}

