//! Claude Code transcripts: `~/.claude/projects/<escaped cwd>/<session id>.jsonl`.

use serde_json::Value;

use super::{Session, SessionMessage, push_merged};
use crate::provider;
use crate::time::parse_rfc3339_ms;
use crate::types::{AgentKind, Role};

pub fn sessions() -> Vec<Session> {
    let Some(projects) = provider::claude::home().map(|h| h.join("projects")) else {
        return vec![];
    };
    let dirs = std::fs::read_dir(projects).into_iter().flatten().flatten();
    // Conversations are `<project>/<session>.jsonl`; sub-agent runs are
    // `<project>/<session>/subagents/*.jsonl`.
    let files = dirs.flat_map(|d| {
        let entries = std::fs::read_dir(d.path()).into_iter().flatten().flatten();
        entries.flat_map(|e| {
            let p = e.path();
            let subagents = p.join("subagents");
            let mut paths = vec![p];
            if subagents.is_dir() {
                paths.extend(
                    std::fs::read_dir(subagents)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .map(|f| f.path()),
                );
            }
            paths
        })
    });
    files
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .filter_map(|p| {
            // Every line of a sub-agent run is a sidechain.
            let subagent = p.parent().is_some_and(|d| d.ends_with("subagents"));
            let mut session = parse(&std::fs::read_to_string(p).ok()?, subagent)?;
            session.subagent = subagent;
            Some(session)
        })
        .collect()
}

/// The prompt a user typed, or None for tool results and CLI bookkeeping
/// (slash commands, local command output).
fn user_text(content: &Value) -> Option<String> {
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let bookkeeping = ["<command-", "<local-command-", "Caveat: The messages below"];
    (!text.trim().is_empty() && !bookkeeping.iter().any(|p| text.starts_with(p))).then_some(text)
}

fn assistant_text(content: &Value) -> String {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Parses one transcript. Subagent (sidechain) and meta entries are skipped.
fn parse(text: &str, sidechain: bool) -> Option<Session> {
    let (mut session_id, mut cwd, mut created_at) = (None, None, None);
    let (mut custom_title, mut ai_title) = (None, None);
    let mut messages: Vec<SessionMessage> = Vec::new();
    for v in text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
    {
        match v["type"].as_str() {
            Some("custom-title") => custom_title = v["customTitle"].as_str().map(String::from),
            Some("ai-title") => ai_title = v["aiTitle"].as_str().map(String::from),
            Some(kind @ ("user" | "assistant")) => {
                if (v["isSidechain"] == true && !sidechain) || v["isMeta"] == true {
                    continue;
                }
                let at = v["timestamp"]
                    .as_str()
                    .and_then(parse_rfc3339_ms)
                    .unwrap_or(0);
                session_id = session_id.or_else(|| v["sessionId"].as_str().map(String::from));
                cwd = cwd.or_else(|| v["cwd"].as_str().map(String::from));
                created_at = created_at.or(Some(at));
                let content = &v["message"]["content"];
                if kind == "user" {
                    if let Some(text) = user_text(content) {
                        push_merged(&mut messages, Role::User, text, at);
                    }
                } else {
                    push_merged(&mut messages, Role::Assistant, assistant_text(content), at);
                }
            }
            _ => {}
        }
    }
    Some(Session {
        agent: AgentKind::Claude,
        session_id: session_id?,
        imported: false,
        cwd: cwd?,
        title: custom_title.or(ai_title),
        model: None,
        created_at: created_at.unwrap_or(0),
        messages,
        subagent: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keeps_prompts_and_replies_only() {
        let lines = [
            json!({"type": "user", "sessionId": "s", "cwd": "/p", "timestamp": "1970-01-01T00:00:01Z",
                "message": {"content": "<command-name>/model</command-name>"}}),
            json!({"type": "user", "sessionId": "s", "cwd": "/p", "message": {"content": "add a test"}}),
            json!({"type": "assistant", "message": {"content": [
                {"type": "thinking", "thinking": "..."}, {"type": "text", "text": "On it"},
                {"type": "tool_use", "id": "t", "name": "Edit", "input": {}}]}}),
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t"}]}}),
            json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "Added"}]}}),
            json!({"type": "assistant", "isSidechain": true, "message": {"content": [{"type": "text", "text": "sub"}]}}),
            json!({"type": "ai-title", "aiTitle": "Add a test"}),
        ];
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        let s = parse(&text.join("\n"), false).unwrap();
        assert_eq!(
            (s.session_id.as_str(), s.cwd.as_str(), s.created_at),
            ("s", "/p", 1000)
        );
        assert_eq!(s.title.as_deref(), Some("Add a test"));
        let got: Vec<_> = s
            .messages
            .iter()
            .map(|m| (m.role, m.text.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (Role::User, "add a test"),
                (Role::Assistant, "On it\n\nAdded")
            ]
        );
    }
}
