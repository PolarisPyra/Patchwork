//! T3 Code threads, from its SQLite state (`~/.t3/userdata/statev2.sqlite`).
//!
//! T3 runs the same agent CLIs, and records each thread's native Claude session
//! or Codex thread id, so imported threads resume the original conversation.
//! The database is only ever opened read-only.

use rusqlite::{Connection, OpenFlags, params};
use serde_json::Value;

use super::{Session, SessionMessage, push_merged};
use crate::provider;
use crate::time::parse_rfc3339_ms;
use crate::types::{AgentKind, Role};

pub fn sessions() -> Vec<Session> {
    let Some(path) = provider::home_dir().map(|h| h.join(".t3/userdata/statev2.sqlite")) else {
        return vec![];
    };
    if !path.is_file() {
        return vec![];
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    Connection::open_with_flags(path, flags)
        .and_then(|conn| read(&conn))
        .unwrap_or_else(|e| {
            log::warn!("couldn't read T3 Code threads: {e}");
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
    let mut threads = conn.prepare(
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

    let rows = threads.query_map([], |r| {
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
        let (thread_id, title, provider, created, root, payload) = row?;
        let Some(agent) = agent_of(&provider) else {
            continue;
        };
        let payload: Value = serde_json::from_str(&payload).unwrap_or_default();
        if payload["lineage"]["relationshipToParent"] == "subagent" {
            continue;
        }
        // The native session id is what lets a follow-up resume the conversation.
        let session_id = native
            .query_map(params![thread_id, provider], |r| r.get::<_, String>(0))?
            .filter_map(Result::ok)
            .filter_map(|p| serde_json::from_str::<Value>(&p).ok())
            .find_map(|p| p["nativeThreadRef"]["nativeId"].as_str().map(String::from));
        let Some(session_id) = session_id else {
            continue;
        };

        let created_at = parse_rfc3339_ms(&created).unwrap_or(0);
        let mut msgs: Vec<SessionMessage> = Vec::new();
        let rows = messages.query_map([&thread_id], |r| {
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
            push_merged(&mut msgs, role, text, at);
        }

        out.push(Session {
            agent,
            session_id,
            imported: false,
            // A thread run in a T3 worktree resumes from that worktree.
            cwd: payload["worktreePath"]
                .as_str()
                .map(String::from)
                .unwrap_or(root),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE projection_projects (project_id TEXT, workspace_root TEXT);
             CREATE TABLE orchestration_v2_projection_threads (thread_id TEXT, project_id TEXT,
                 title TEXT, default_provider TEXT, created_at TEXT, payload_json TEXT,
                 deleted_at TEXT, archived_at TEXT);
             CREATE TABLE orchestration_v2_projection_provider_threads (thread_id TEXT,
                 provider TEXT, payload_json TEXT, updated_at TEXT);
             CREATE TABLE orchestration_v2_projection_messages (thread_id TEXT, role TEXT,
                 streaming INTEGER, created_at TEXT, payload_json TEXT);
             INSERT INTO projection_projects VALUES ('p', '/repo');",
        )
        .unwrap();
        conn
    }

    fn thread(conn: &Connection, id: &str, provider: &str, payload: &str, native: Option<&str>) {
        conn.execute(
            "INSERT INTO orchestration_v2_projection_threads
             VALUES (?1, 'p', 'Fix the build', ?2, '1970-01-01T00:00:01Z', ?3, NULL, NULL)",
            params![id, provider, payload],
        )
        .unwrap();
        if let Some(native) = native {
            let p = format!(r#"{{"nativeThreadRef":{{"nativeId":"{native}"}}}}"#);
            conn.execute(
                "INSERT INTO orchestration_v2_projection_provider_threads VALUES (?1, ?2, ?3, 'x')",
                params![id, provider, p],
            )
            .unwrap();
        }
    }

    #[test]
    fn reads_resumable_claude_and_codex_threads() {
        let conn = db();
        thread(
            &conn,
            "a",
            "claudeAgent",
            r#"{"modelSelection":{"model":"claude-opus-5-5"},"worktreePath":"/wt"}"#,
            Some("sess-1"),
        );
        thread(&conn, "b", "codex", "{}", Some("codex-thread"));
        thread(&conn, "c", "antigravity", "{}", Some("x"));
        thread(
            &conn,
            "d",
            "codex",
            r#"{"lineage":{"relationshipToParent":"subagent"}}"#,
            Some("y"),
        );
        thread(&conn, "e", "codex", "{}", None);
        conn.execute_batch(
            r#"INSERT INTO orchestration_v2_projection_messages VALUES
               ('a', 'user', 0, '1', '{"text":"fix it","createdAt":"1970-01-01T00:00:02Z"}'),
               ('a', 'assistant', 0, '2', '{"text":"Done"}'),
               ('a', 'assistant', 1, '3', '{"text":"still streaming"}');"#,
        )
        .unwrap();

        let mut sessions = read(&conn).unwrap();
        sessions.sort_by(|x, y| x.session_id.cmp(&y.session_id));
        let ids: Vec<_> = sessions
            .iter()
            .map(|s| (s.agent, s.session_id.as_str()))
            .collect();
        assert_eq!(
            ids,
            [
                (AgentKind::Codex, "codex-thread"),
                (AgentKind::Claude, "sess-1")
            ]
        );

        let a = &sessions[1];
        assert_eq!(
            (a.cwd.as_str(), a.model.as_deref()),
            ("/wt", Some("claude-opus-5-5"))
        );
        assert_eq!(
            (a.title.as_deref(), a.created_at),
            (Some("Fix the build"), 1000)
        );
        let got: Vec<_> = a
            .messages
            .iter()
            .map(|m| (m.role, m.text.as_str()))
            .collect();
        assert_eq!(got, [(Role::User, "fix it"), (Role::Assistant, "Done")]);
        assert_eq!(sessions[0].cwd, "/repo");
    }
}
