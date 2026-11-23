//! T3 Code chats, from its SQLite state (`~/.t3/userdata/statev2.sqlite`).
//!
//! T3 runs the same agent CLIs, and records each chat's native Claude session
//! or Codex chat id, so imported chats resume the original conversation.
//! The database is only ever opened read-only.

// Rust guideline compliant 2026-10-09

use rusqlite::{Connection, OpenFlags, params};
use serde_json::Value;

use super::{Session, SessionMessage, ilog, push_merged};
use crate::system::time::parse_rfc3339_ms;
use crate::provider::AgentKind;
use crate::chats::Role;

pub fn sessions() -> Vec<Session> {
    let Some(path) =
        crate::system::files::paths::home_dir().map(|h| h.join(".t3/userdata/statev2.sqlite"))
    else {
        ilog!("T3: no home folder");
        return vec![];
    };
    ilog!(
        "T3: reading {} (exists: {})",
        path.display(),
        path.is_file()
    );
    if !path.is_file() {
        return vec![];
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    Connection::open_with_flags(path, flags)
        .and_then(|conn| read(&conn))
        .unwrap_or_else(|e| {
            log::warn!(error:% = e; "couldn't read T3 Code threads: {{error}}");
            ilog!("T3: couldn't read the database: {e}");
            vec![]
        })
}

/// T3's provider names for the agents Patchwork can resume.
fn agent_of(provider: &str) -> Option<AgentKind> {
    match provider {
        "claudeAgent" => Some(AgentKind::Claude),
        "codex" => Some(AgentKind::Codex),
        _ => None,
    }
}

fn read(conn: &Connection) -> rusqlite::Result<Vec<Session>> {
    let mut chats = conn.prepare(
        "SELECT t.thread_id, t.title, t.default_provider, t.created_at, p.workspace_root,
                t.payload_json
         FROM orchestration_v2_projection_threads t
         JOIN projection_projects p ON p.project_id = t.project_id
         WHERE t.deleted_at IS NULL AND t.archived_at IS NULL",
    )?;
    let mut native = conn.prepare(
        "SELECT payload_json FROM orchestration_v2_projection_provider_threads
         WHERE thread_id = ?1 AND provider = ?2 ORDER BY updated_at DESC",
    )?;
    let mut messages = conn.prepare(
        "SELECT role, payload_json FROM orchestration_v2_projection_messages
         WHERE thread_id = ?1 AND streaming = 0 ORDER BY created_at",
    )?;

    let rows = chats.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (chat_id, title, provider, created, root, payload) = row?;
        let Some(agent) = agent_of(&provider) else {
            ilog!("T3: skipped {chat_id}: provider {provider} can't be resumed");
            continue;
        };
        let payload: Value = serde_json::from_str(&payload).unwrap_or_default();
        if payload["lineage"]["relationshipToParent"] == "subagent" {
            continue;
        }
        // The native session id is what lets a follow-up resume the conversation.
        let session_id = native
            .query_map(params![chat_id, provider], |r| r.get::<_, String>(0))?
            .filter_map(Result::ok)
            .filter_map(|p| serde_json::from_str::<Value>(&p).ok())
            .find_map(|p| p["nativeThreadRef"]["nativeId"].as_str().map(String::from));
        let Some(session_id) = session_id else {
            ilog!("T3: skipped {chat_id}: no native session id");
            continue;
        };

        let created_at = parse_rfc3339_ms(&created).unwrap_or(0);
        let mut msgs: Vec<SessionMessage> = Vec::new();
        let rows = messages.query_map([&chat_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (role, payload) = row?;
            let role = match role.as_str() {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                _ => continue,
            };
            let m: Value = serde_json::from_str(&payload).unwrap_or_default();
            let at = m["createdAt"]
                .as_str()
                .and_then(parse_rfc3339_ms)
                .unwrap_or(created_at);
            let text = m["text"].as_str().unwrap_or_default().to_string();
            push_merged(&mut msgs, role, &text, at);
        }

        out.push(Session {
            agent,
            session_id,
            imported: false,
            // A chat run in a T3 worktree resumes from that worktree.
            cwd: payload["worktreePath"].as_str().map_or(root, String::from),
            title: Some(title).filter(|t| t != "New thread"),
            model: payload["modelSelection"]["model"]
                .as_str()
                .map(String::from),
            created_at,
            messages: msgs,
            subagent: false,
        });
    }
    Ok(out)
}
