//! Codex rollout files: `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl`.
//! Archived sessions live elsewhere and are not imported.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use super::{Session, SessionMessage, ilog, push_merged};
use crate::provider;
use crate::system::time::parse_rfc3339_ms;
use crate::provider::AgentKind;
use crate::chats::Role;

pub fn sessions() -> Vec<Session> {
    let Some(home) = provider::codex::home() else {
        ilog!("Codex: no home folder");
        return vec![];
    };
    let titles = chat_names(&home.join("session_index.jsonl"));
    let dir = home.join("sessions");
    let files = provider::codex::rollout_files(&dir);
    ilog!(
        "Codex: reading {} (exists: {}), {} rollout files",
        dir.display(),
        dir.is_dir(),
        files.len()
    );
    files
        .into_iter()
        .filter_map(|(_, path)| {
            let file = File::open(&path)
                .map_err(|e| ilog!("Codex: couldn't read {}: {e}", path.display()))
                .ok()?;
            let Some(mut session) = parse(BufReader::new(file)) else {
                ilog!(
                    "Codex: skipped {}: no session_meta with an id and folder",
                    path.display()
                );
                return None;
            };
            session.title = titles.get(&session.session_id).cloned();
            Some(session)
        })
        .collect()
}

/// Chat names Codex generated, from `session_index.jsonl` (later lines win).
fn chat_names(path: &Path) -> HashMap<String, String> {
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
    let user_chat = payload["thread_source"]
        .as_str()
        .is_none_or(|s| s == "user");
    let subagent = !user_chat || payload["source"].is_object();
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
                        &joined_text(&item["content"], "text"),
                        at,
                    ),
                    Some("AgentMessage") => push_merged(
                        &mut items,
                        Role::Assistant,
                        &joined_text(&item["content"], "Text"),
                        at,
                    ),
                    _ => {}
                }
            }
            Some("user_message") => {
                let text = p["message"].as_str().unwrap_or_default().to_string();
                push_merged(&mut legacy, Role::User, &text, at);
            }
            Some("agent_message") => {
                let text = p["message"].as_str().unwrap_or_default().to_string();
                push_merged(&mut legacy, Role::Assistant, &text, at);
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
