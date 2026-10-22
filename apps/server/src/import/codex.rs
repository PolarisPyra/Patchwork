//! Codex rollout files: `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl`.
//! Archived sessions live elsewhere and are not imported.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use super::{Session, SessionMessage, push_merged};
use crate::provider;
use crate::time::parse_rfc3339_ms;
use crate::types::{AgentKind, Role};

pub fn sessions() -> Vec<Session> {
    let Some(home) = provider::codex::home() else {
        return vec![];
    };
    let titles = thread_names(&home.join("session_index.jsonl"));
    provider::codex::rollout_files(&home.join("sessions"))
        .into_iter()
        .filter_map(|(_, path)| {
            let mut session = parse(BufReader::new(File::open(&path).ok()?))?;
            session.title = titles.get(&session.session_id).cloned();
            Some(session)
        })
        .collect()
}

/// Thread names Codex generated, from `session_index.jsonl` (later lines win).
fn thread_names(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| Some((v["id"].as_str()?.into(), v["thread_name"].as_str()?.into())))
        .collect()
}

fn joined_text(content: &Value, block_type: &str) -> String {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["type"] == block_type)
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parses one rollout file, line by line. Subagent sessions (e.g. guardian reviews) are flagged.
fn parse(reader: impl BufRead) -> Option<Session> {
    let mut lines = reader.lines().map_while(Result::ok);
    let meta = lines
        .by_ref()
        .find_map(|l| serde_json::from_str::<Value>(&l).ok())
        .filter(|v| v["type"] == "session_meta")?;
    let payload = &meta["payload"];
    let user_thread = payload["thread_source"]
        .as_str()
        .is_none_or(|s| s == "user");
    let subagent = !user_thread || payload["source"].is_object();
    let session_id = payload["id"].as_str()?.to_string();
    let cwd = payload["cwd"].as_str()?.to_string();
    let created_at = payload["timestamp"]
        .as_str()
        .and_then(parse_rfc3339_ms)
        .unwrap_or(0);

    // Newer CLIs record whole items; older ones record user_message / agent_message events.
    let (mut items, mut legacy) = (Vec::new(), Vec::new());
    // Most lines are large tool output; only event lines are worth parsing.
    let events = lines
        .filter(|l| l.contains("\"event_msg\""))
        .filter_map(|l| serde_json::from_str::<Value>(&l).ok())
        .filter(|v| v["type"] == "event_msg");
    for v in events {
        let at = v["timestamp"]
            .as_str()
            .and_then(parse_rfc3339_ms)
            .unwrap_or(created_at);
        let p = &v["payload"];
        match p["type"].as_str() {
            Some("item_completed") => {
                let item = &p["item"];
                match item["type"].as_str() {
                    Some("UserMessage") => push_merged(
                        &mut items,
                        Role::User,
                        joined_text(&item["content"], "text"),
                        at,
                    ),
                    Some("AgentMessage") => push_merged(
                        &mut items,
                        Role::Assistant,
                        joined_text(&item["content"], "Text"),
                        at,
                    ),
                    _ => {}
                }
            }
            Some("user_message") => {
                let text = p["message"].as_str().unwrap_or_default().to_string();
                push_merged(&mut legacy, Role::User, text, at);
            }
            Some("agent_message") => {
                let text = p["message"].as_str().unwrap_or_default().to_string();
                push_merged(&mut legacy, Role::Assistant, text, at);
            }
            _ => {}
        }
    }
    let messages: Vec<SessionMessage> = if items.is_empty() { legacy } else { items };
    Some(Session {
        agent: AgentKind::Codex,
        session_id,
        imported: false,
        cwd,
        title: None,
        model: None,
        created_at,
        messages,
        subagent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(lines: &[Value]) -> String {
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn reads_items_and_legacy_events() {
        let meta = json!({"type": "session_meta", "payload": {"id": "s1", "cwd": "/p",
            "timestamp": "1970-01-01T00:00:01Z", "source": "cli", "thread_source": "user"}});
        let items = file(&[
            meta.clone(),
            json!({"type": "event_msg", "timestamp": "1970-01-01T00:00:02Z", "payload": {"type": "item_completed",
                "item": {"type": "UserMessage", "content": [{"type": "text", "text": "fix it"}]}}}),
            json!({"type": "event_msg", "payload": {"type": "item_completed",
                "item": {"type": "AgentMessage", "content": [{"type": "Text", "text": "Looking"}]}}}),
            json!({"type": "event_msg", "payload": {"type": "item_completed",
                "item": {"type": "AgentMessage", "content": [{"type": "Text", "text": "Done"}]}}}),
        ]);
        let s = parse(items.as_bytes()).unwrap();
        assert_eq!(
            (s.session_id.as_str(), s.cwd.as_str(), s.created_at),
            ("s1", "/p", 1000)
        );
        assert_eq!(s.messages.len(), 2);
        assert_eq!(
            (s.messages[0].role, s.messages[0].created_at),
            (Role::User, 2000)
        );
        assert_eq!(s.messages[1].text, "Looking\n\nDone");

        let legacy = file(&[
            meta,
            json!({"type": "event_msg", "payload": {"type": "user_message", "message": "hi"}}),
            json!({"type": "event_msg", "payload": {"type": "agent_message", "message": "hello"}}),
        ]);
        assert_eq!(parse(legacy.as_bytes()).unwrap().messages.len(), 2);
    }

    #[test]
    fn flags_subagent_sessions() {
        let sub = json!({"type": "session_meta", "payload": {"id": "s", "cwd": "/p",
            "source": {"subagent": {"other": "guardian"}}, "thread_source": "guardian_review"}});
        assert!(parse(sub.to_string().as_bytes()).is_some_and(|s| s.subagent));
    }
}
