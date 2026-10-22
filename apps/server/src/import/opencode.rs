//! OpenCode sessions, from its SQLite database (`~/.local/share/opencode/opencode.db`).
//!
//! OpenCode is not an agent Patchwork runs, so these conversations cannot be resumed. They are
//! imported as transcripts, which the first turn of the thread sends to the agent as context.
//! The database is only ever opened read-only.

use rusqlite::{Connection, OpenFlags, params};
use serde_json::Value;

use super::{Session, SessionMessage, push_merged};
use crate::provider;
use crate::types::{AgentKind, Role};

pub fn sessions(agent: AgentKind) -> Vec<Session> {
    let Some(path) = provider::home_dir().map(|h| h.join(".local/share/opencode/opencode.db"))
    else {
        return vec![];
    };
    if !path.is_file() {
        return vec![];
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    Connection::open_with_flags(path, flags)
        .and_then(|conn| read(&conn, agent))
        .unwrap_or_else(|e| {
            log::warn!("couldn't read OpenCode sessions: {e}");
            vec![]
        })
}

fn read(conn: &Connection, agent: AgentKind) -> rusqlite::Result<Vec<Session>> {
    let mut sessions = conn.prepare(
        "SELECT id, title, directory, time_created, parent_id IS NOT NULL
         FROM session WHERE time_archived IS NULL",
    )?;
    let rows = sessions.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, bool>(4)?,
        ))
    })?;
    rows.map(|row| {
        let (id, title, cwd, created, subagent) = row?;
        let messages = session_messages(conn, &id)?;
        Ok(Session {
            agent,
            session_id: id,
            imported: true,
            cwd,
            title: Some(title),
            model: None,
            created_at: created as u64,
            messages,
            subagent,
        })
    })
    .collect()
}

/// A session's visible text, in order. Tool calls, reasoning and the instructions a harness
/// adds (marked synthetic) are left out.
fn session_messages(conn: &Connection, session: &str) -> rusqlite::Result<Vec<SessionMessage>> {
    let mut messages = conn.prepare(
        "SELECT id, data, time_created FROM message WHERE session_id = ?1 ORDER BY time_created",
    )?;
    let mut parts = conn.prepare(
        "SELECT data FROM part WHERE message_id = ?1 ORDER BY time_created, id",
    )?;
    let mut out = Vec::new();
    let rows = messages.query_map(params![session], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (id, data, at) = row?;
        let Ok(info) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        let role = match info["role"].as_str() {
            Some("user") => Role::User,
            Some("assistant") => Role::Assistant,
            _ => continue,
        };
        let mut texts = Vec::new();
        for part in parts.query_map(params![id], |r| r.get::<_, String>(0))? {
            let part = part?;
            let Ok(p) = serde_json::from_str::<Value>(&part) else {
                continue;
            };
            if p["type"] == "text" && p["synthetic"] != true && p["ignored"] != true {
                texts.extend(p["text"].as_str().map(String::from));
            }
        }
        push_merged(&mut out, role, texts.join("\n\n"), at as u64);
    }
    Ok(out)
}
