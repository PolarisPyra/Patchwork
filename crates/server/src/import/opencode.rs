//! OpenCode sessions, from its SQLite database (`~/.local/share/opencode/opencode.db`): the 1.x
//! `session`/`message`/`part` tables and the 2.x `session_v2`/`session_message` tables.
//!
//! OpenCode is not an agent Patchwork runs, so these conversations cannot be resumed. They are
//! imported as transcripts, which the first turn of the thread sends to the agent as context.
//! The database is only ever opened read-only.

// Rust guideline compliant 2026-10-09

use rusqlite::{Connection, OpenFlags, params};
use serde_json::Value;

use super::{Session, SessionMessage, ilog, push_merged};
use crate::provider::AgentKind;
use crate::threads::Role;

pub fn sessions(agent: AgentKind) -> Vec<Session> {
    let Some(path) =
        crate::system::fs::home_dir().map(|h| h.join(".local/share/opencode/opencode.db"))
    else {
        ilog!("OpenCode: no home folder");
        return vec![];
    };
    ilog!(
        "OpenCode: reading {} (exists: {})",
        path.display(),
        path.is_file()
    );
    if !path.is_file() {
        return vec![];
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    Connection::open_with_flags(path, flags)
        .and_then(|conn| read(&conn, agent))
        .unwrap_or_else(|e| {
            log::warn!(error:% = e; "couldn't read OpenCode sessions: {{error}}");
            ilog!("OpenCode: couldn't read the database: {e}");
            vec![]
        })
}

fn read(conn: &Connection, agent: AgentKind) -> rusqlite::Result<Vec<Session>> {
    let legacy = has_table(conn, "session")?;
    let v2 = has_table(conn, "session_v2")?;
    // OpenCode 2.x keeps sessions in `session_v2`, and only some of them also in `session`.
    let query = match (legacy, v2) {
        (true, true) => {
            "SELECT id, title, directory, time_created, parent_id IS NOT NULL
             FROM session WHERE time_archived IS NULL
             UNION ALL
             SELECT id, title, directory, time_created, parent_id IS NOT NULL
             FROM session_v2 WHERE time_archived IS NULL AND id NOT IN (SELECT id FROM session)"
        }
        (true, false) => {
            "SELECT id, title, directory, time_created, parent_id IS NOT NULL
             FROM session WHERE time_archived IS NULL"
        }
        (false, true) => {
            "SELECT id, title, directory, time_created, parent_id IS NOT NULL
             FROM session_v2 WHERE time_archived IS NULL"
        }
        (false, false) => {
            ilog!("OpenCode: no session or session_v2 table");
            return Ok(vec![]);
        }
    };
    let v2_messages = has_table(conn, "session_message")?;
    let legacy_messages = has_table(conn, "message")? && has_table(conn, "part")?;
    ilog!(
        "OpenCode: tables: session {legacy}, session_v2 {v2}, message/part {legacy_messages}, session_message {v2_messages}"
    );
    let mut sessions = conn.prepare(query)?;
    let rows = sessions.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, bool>(4)?,
        ))
    })?;
    rows.map(|row| {
        let (id, title, cwd, created, subagent) = row?;
        let mut messages = if v2_messages {
            v2_session_messages(conn, &id)?
        } else {
            Vec::new()
        };
        let mut from = "session_message";
        if messages.is_empty() && legacy_messages {
            messages = session_messages(conn, &id)?;
            from = "message/part";
        }
        ilog!(
            "OpenCode: {id} ({cwd}): {} messages from {from}",
            messages.len()
        );
        Ok(Session {
            agent,
            session_id: id,
            imported: true,
            cwd,
            title,
            model: None,
            created_at: created as u64,
            messages,
            subagent,
        })
    })
    .collect()
}

fn has_table(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        params![name],
        |r| r.get(0),
    )
}

/// A 2.x session's visible text, in order: user prompts and the text the assistant wrote.
/// Tool calls, reasoning, compaction summaries and harness-added messages are left out.
fn v2_session_messages(conn: &Connection, session: &str) -> rusqlite::Result<Vec<SessionMessage>> {
    let mut messages = conn.prepare(
        "SELECT type, data, time_created FROM session_message
         WHERE session_id = ?1 AND type IN ('user', 'assistant') ORDER BY seq",
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
        let (kind, data, at) = row?;
        let Ok(info) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        let (role, text) = if kind == "user" {
            (
                Role::User,
                info["text"].as_str().unwrap_or_default().to_string(),
            )
        } else {
            let texts: Vec<&str> = info["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|c| c["type"] == "text")
                .filter_map(|c| c["text"].as_str())
                .collect();
            (Role::Assistant, texts.join("\n\n"))
        };
        push_merged(&mut out, role, &text, at as u64);
    }
    Ok(out)
}

/// A 1.x session's visible text, in order. Tool calls, reasoning and the instructions a harness
/// adds (marked synthetic) are left out.
fn session_messages(conn: &Connection, session: &str) -> rusqlite::Result<Vec<SessionMessage>> {
    let mut messages = conn.prepare(
        "SELECT id, data, time_created FROM message WHERE session_id = ?1 ORDER BY time_created",
    )?;
    let mut parts =
        conn.prepare("SELECT data FROM part WHERE message_id = ?1 ORDER BY time_created, id")?;
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
        push_merged(&mut out, role, &texts.join("\n\n"), at as u64);
    }
    Ok(out)
}
