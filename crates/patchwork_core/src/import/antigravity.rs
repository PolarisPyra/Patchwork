//! Gemini CLI chats: `~/.gemini/tmp/<project>/chats/session-*.jsonl` (older
//! versions wrote one `.json` document). `~/.gemini/projects.json` maps each
//! project directory to its folder name under `tmp/`. agy's own conversations
//! are read by `agy` and imported along with these.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use super::{Session, SessionMessage, agy, ilog, push_merged};
use crate::provider;
use crate::system::time::parse_rfc3339_ms;
use crate::provider::AgentKind;
use crate::chats::Role;

/// agy's own conversations, then the chats the Gemini CLI saved before it.
pub fn sessions() -> Vec<Session> {
    let mut out = agy::sessions();
    out.extend(gemini_cli_sessions());
    out
}

fn gemini_cli_sessions() -> Vec<Session> {
    let Some(home) = provider::antigravity::home() else {
        ilog!("Antigravity: no home folder");
        return vec![];
    };
    let roots = project_roots(&home);
    let tmp = home.join("tmp");
    ilog!(
        "Antigravity: reading {} (exists: {}), {} projects in projects.json",
        tmp.display(),
        tmp.is_dir(),
        roots.len()
    );
    let dirs = std::fs::read_dir(tmp).into_iter().flatten().flatten();
    let mut out = Vec::new();
    for dir in dirs {
        let name = dir.file_name().to_string_lossy().into_owned();
        let Some(cwd) = roots
            .get(&name)
            .cloned()
            .or_else(|| marker_root(&dir.path()))
        else {
            ilog!("Antigravity: skipped folder {name}: no project folder recorded for it");
            continue;
        };
        let chats = std::fs::read_dir(dir.path().join("chats"))
            .into_iter()
            .flatten()
            .flatten();
        for chat in chats.map(|c| c.path()).filter(|p| p.is_file()) {
            let text = match std::fs::read_to_string(&chat) {
                Ok(text) => text,
                Err(e) => {
                    ilog!("Antigravity: couldn't read {}: {e}", chat.display());
                    continue;
                }
            };
            let parsed = if chat.extension().is_some_and(|e| e == "json") {
                serde_json::from_str(&text)
                    .ok()
                    .and_then(|v| parse_document(&v, &cwd))
            } else {
                parse_jsonl(&text, &cwd)
            };
            if parsed.is_none() {
                ilog!(
                    "Antigravity: skipped {}: unrecognized chat file",
                    chat.display()
                );
            }
            out.extend(parsed);
        }
    }
    out
}

/// Folder name under `tmp/` → project directory, from `projects.json`.
fn project_roots(home: &Path) -> HashMap<String, String> {
    let v: Value = std::fs::read_to_string(home.join("projects.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    v["projects"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(path, slug)| Some((slug.as_str()?.to_string(), path.clone())))
        .collect()
}

/// Project directory recorded in a `.project_root` marker inside the folder, if any.
fn marker_root(dir: &Path) -> Option<String> {
    let root = std::fs::read_to_string(dir.join(".project_root")).ok()?;
    Some(root.trim().to_string()).filter(|r| !r.is_empty())
}

/// Text of a message's `content`: a string or a list of `{text}` parts.
fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn push_record(messages: &mut Vec<SessionMessage>, record: &Value, fallback_at: u64) {
    let role = match record["type"].as_str() {
        Some("user") => Role::User,
        Some("gemini") => Role::Assistant,
        _ => return,
    };
    let at = record["timestamp"]
        .as_str()
        .and_then(parse_rfc3339_ms)
        .unwrap_or(fallback_at);
    push_merged(messages, role, &content_text(&record["content"]), at);
}

fn session(meta: &Value, cwd: &str, messages: Vec<SessionMessage>) -> Option<Session> {
    Some(Session {
        agent: AgentKind::Antigravity,
        session_id: meta["sessionId"].as_str()?.to_string(),
        imported: false,
        cwd: cwd.to_string(),
        title: None,
        model: None,
        created_at: meta["startTime"]
            .as_str()
            .and_then(parse_rfc3339_ms)
            .unwrap_or(0),
        messages,
        subagent: false,
    })
}

/// Current format: a metadata line, then one record per line. Update and
/// rewind operations (`$set`, `$patch`, `$rewindTo`) are not replayed.
fn parse_jsonl(text: &str, cwd: &str) -> Option<Session> {
    let mut lines = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok());
    let meta = lines.next()?;
    let start = meta["startTime"]
        .as_str()
        .and_then(parse_rfc3339_ms)
        .unwrap_or(0);
    let mut messages = Vec::new();
    lines.for_each(|record| push_record(&mut messages, &record, start));
    session(&meta, cwd, messages)
}

/// Older format: one JSON document with a `messages` array.
fn parse_document(v: &Value, cwd: &str) -> Option<Session> {
    let start = v["startTime"]
        .as_str()
        .and_then(parse_rfc3339_ms)
        .unwrap_or(0);
    let mut messages = Vec::new();
    v["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .for_each(|record| push_record(&mut messages, record, start));
    session(v, cwd, messages)
}
