//! Gemini CLI chats: `~/.gemini/tmp/<project>/chats/session-*.jsonl` (older
//! versions wrote one `.json` document). `~/.gemini/projects.json` maps each
//! project directory to its folder name under `tmp/`.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use super::{Session, SessionMessage, push_merged};
use crate::provider;
use crate::time::parse_rfc3339_ms;
use crate::types::{AgentKind, Role};

pub fn sessions() -> Vec<Session> {
    let Some(home) = provider::antigravity::home() else {
        return vec![];
    };
    let roots = project_roots(&home);
    let dirs = std::fs::read_dir(home.join("tmp"))
        .into_iter()
        .flatten()
        .flatten();
    let mut out = Vec::new();
    for dir in dirs {
        let name = dir.file_name().to_string_lossy().into_owned();
        let Some(cwd) = roots
            .get(&name)
            .cloned()
            .or_else(|| marker_root(&dir.path()))
        else {
            continue;
        };
        let chats = std::fs::read_dir(dir.path().join("chats"))
            .into_iter()
            .flatten()
            .flatten();
        for chat in chats.map(|c| c.path()).filter(|p| p.is_file()) {
            let Ok(text) = std::fs::read_to_string(&chat) else {
                continue;
            };
            let parsed = if chat.extension().is_some_and(|e| e == "json") {
                serde_json::from_str(&text)
                    .ok()
                    .and_then(|v| parse_document(&v, &cwd))
            } else {
                parse_jsonl(&text, &cwd)
            };
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
    push_merged(messages, role, content_text(&record["content"]), at);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_both_formats() {
        let lines = [
            json!({"sessionId": "g1", "startTime": "1970-01-01T00:00:01Z", "kind": "main"}),
            json!({"id": "1", "type": "user", "content": [{"text": "hi"}], "timestamp": "1970-01-01T00:00:02Z"}),
            json!({"id": "2", "type": "info", "content": "note"}),
            json!({"id": "3", "type": "gemini", "content": "hello"}),
            json!({"$set": {"lastUpdated": "x"}}),
        ];
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        let s = parse_jsonl(&text.join("\n"), "/p").unwrap();
        assert_eq!((s.session_id.as_str(), s.created_at), ("g1", 1000));
        let got: Vec<_> = s
            .messages
            .iter()
            .map(|m| (m.role, m.text.as_str(), m.created_at))
            .collect();
        assert_eq!(
            got,
            [(Role::User, "hi", 2000), (Role::Assistant, "hello", 1000)]
        );

        let doc = json!({"sessionId": "g2", "messages": [{"type": "user", "content": "yo"}]});
        assert_eq!(parse_document(&doc, "/p").unwrap().messages.len(), 1);
    }
}
