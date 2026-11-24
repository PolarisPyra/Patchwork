//! agy's own conversations, the ones `agy` and `agy acp` resume. Its index,
//! `~/.gemini/antigravity-cli/conversation_summaries.db`, names each conversation's title,
//! folders and parent; `brain/<id>/.system_generated/logs/transcript.jsonl` holds its steps.
//!
//! The index is only ever opened read-only.

// Rust guideline compliant 2026-10-09

use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use url::Url;

use super::{Session, SessionMessage, ilog, push_merged};
use crate::chats::Role;
use crate::provider::{self, AgentKind};
use crate::system::time::parse_rfc3339_ms;

/// The agy profile `agy` and `agy acp` keep their conversations in.
const PROFILE: &str = "antigravity-cli";

/// Where a conversation's transcript is, under its folder in `brain/`.
const TRANSCRIPT: &str = ".system_generated/logs/transcript.jsonl";

/// The tags agy wraps the user's own words in, among the context it adds to each message.
const REQUEST_OPEN: &str = "<USER_REQUEST>";
const REQUEST_CLOSE: &str = "</USER_REQUEST>";

/// One conversation in agy's index.
struct Summary {
    id: String,
    title: String,
    /// JSON array of the `file://` URIs of the conversation's folders.
    workspaces: String,
    /// The conversation that started this one as a sub agent or swarm worker; empty if none.
    parent: String,
}

pub(super) fn sessions() -> Vec<Session> {
    let Some(dir) = provider::antigravity::home().map(|h| h.join(PROFILE)) else {
        ilog!("agy: no home folder");
        return vec![];
    };
    let index = dir.join("conversation_summaries.db");
    ilog!(
        "agy: reading {} (exists: {})",
        index.display(),
        index.is_file()
    );
    if !index.is_file() {
        return vec![];
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let summaries = Connection::open_with_flags(&index, flags)
        .and_then(|conn| read(&conn))
        .unwrap_or_else(|e| {
            log::warn!(error:% = e; "couldn't read agy's conversation index: {{error}}");
            ilog!("agy: couldn't read the index: {e}");
            vec![]
        });
    let brain = dir.join("brain");
    summaries
        .into_iter()
        .filter_map(|s| session(&brain, s))
        .collect()
}

fn read(conn: &Connection) -> rusqlite::Result<Vec<Summary>> {
    let mut stmt = conn.prepare(
        "SELECT conversation_id, title, workspace_uris, parent_conversation_id
         FROM conversation_summaries WHERE step_count > 0",
    )?;
    stmt.query_map([], |row| {
        Ok(Summary {
            id: row.get(0)?,
            title: row.get(1)?,
            workspaces: row.get(2)?,
            parent: row.get(3)?,
        })
    })?
    .collect()
}

fn session(brain: &Path, summary: Summary) -> Option<Session> {
    // The id becomes a path segment, so it must not be able to climb out of `brain/`.
    let safe = !summary.id.is_empty()
        && summary
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !safe {
        ilog!("agy: skipped {:?}: not a conversation id", summary.id);
        return None;
    }
    let Some(cwd) = folder(&summary.workspaces) else {
        ilog!("agy: skipped {}: no folder recorded for it", summary.id);
        return None;
    };
    let path = brain.join(&summary.id).join(TRANSCRIPT);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            ilog!(
                "agy: skipped {}: couldn't read its transcript: {e}",
                summary.id
            );
            return None;
        }
    };
    let (messages, created_at) = parse_transcript(&text);
    Some(Session {
        agent: AgentKind::Antigravity,
        subagent: !summary.parent.is_empty(),
        session_id: summary.id,
        imported: false,
        cwd,
        title: Some(summary.title).filter(|t| !t.trim().is_empty()),
        model: None,
        created_at,
        messages,
    })
}

/// The conversation's folder: the first of its workspaces, which is the one agy was started in.
fn folder(workspaces: &str) -> Option<String> {
    let uris: Vec<String> = serde_json::from_str(workspaces).ok()?;
    uris.iter()
        .filter_map(|uri| Url::parse(uri).ok()?.to_file_path().ok())
        .map(|path| path.to_string_lossy().into_owned())
        .next()
}

/// The user's requests and the model's replies, and when the conversation started.
fn parse_transcript(text: &str) -> (Vec<SessionMessage>, u64) {
    let mut messages = Vec::new();
    let mut started = None;
    for record in text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
    {
        let at = record["created_at"].as_str().and_then(parse_rfc3339_ms);
        started = started.or(at);
        let at = at.or(started).unwrap_or(0);
        let Some(content) = record["content"].as_str() else {
            continue;
        };
        match (record["source"].as_str(), record["type"].as_str()) {
            (Some("USER_EXPLICIT"), Some("USER_INPUT")) => {
                if let Some(request) = user_request(content) {
                    push_merged(&mut messages, Role::User, request, at);
                }
            }
            (Some("MODEL"), Some("PLANNER_RESPONSE")) => {
                push_merged(&mut messages, Role::Assistant, content, at);
            }
            _ => {}
        }
    }
    (messages, started.unwrap_or(0))
}

/// What the user typed, without the context agy adds around it.
fn user_request(content: &str) -> Option<&str> {
    let start = content.find(REQUEST_OPEN)? + REQUEST_OPEN.len();
    let rest = &content[start..];
    Some(rest.find(REQUEST_CLOSE).map_or(rest, |end| &rest[..end]))
}
