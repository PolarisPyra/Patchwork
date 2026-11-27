//! Running tool call state as ACP updates arrive.
//!
//! A `tool_call_update` or permission request may carry only what changed, so each is filled in
//! from what the call already had.

// Rust guideline compliant 2026-11-23

use std::collections::HashMap;

use serde_json::{Map, Value};

/// The largest field, as JSON, kept to fill in a call's later updates.
const MAX_REMEMBERED: usize = 64 << 10;

/// Each running call's fields so far, by its `toolCallId`.
#[derive(Debug, Default)]
pub struct ToolCalls {
    calls: HashMap<String, Map<String, Value>>,
}

impl ToolCalls {
    /// Fills `line` in from the call it names, and notes what it adds.
    ///
    /// A `tool_call` or `tool_call_update` becomes the whole call so far; a call that has ended
    /// is forgotten. A `session/request_permission` gets the call's fields it leaves out.
    pub fn merge(&mut self, line: &mut Value) {
        match line["method"].as_str() {
            Some("session/update") => self.update(&mut line["params"]["update"]),
            Some("session/request_permission") => {
                let tool = &mut line["params"]["toolCall"];
                let Some(known) = tool["toolCallId"]
                    .as_str()
                    .and_then(|id| self.calls.get(id))
                else {
                    return;
                };
                let Some(tool) = tool.as_object_mut() else {
                    return;
                };
                for (key, value) in known {
                    if tool.get(key).is_none_or(Value::is_null) {
                        tool.insert(key.clone(), value.clone());
                    }
                }
            }
            _ => {}
        }
    }

    fn update(&mut self, update: &mut Value) {
        if !matches!(
            update["sessionUpdate"].as_str(),
            Some("tool_call" | "tool_call_update")
        ) {
            return;
        }
        let Some(id) = update["toolCallId"].as_str().map(String::from) else {
            return;
        };
        let Some(fields) = update.as_object() else {
            return;
        };
        let call = self.calls.entry(id.clone()).or_default();
        let mut whole = call.clone();
        for (key, value) in fields.iter().filter(|(_, v)| !v.is_null()) {
            whole.insert(key.clone(), value.clone());
            if value.to_string().len() <= MAX_REMEMBERED {
                call.insert(key.clone(), value.clone());
            } else {
                call.remove(key);
            }
        }
        let ended = matches!(
            whole.get("status").and_then(Value::as_str),
            Some("completed" | "failed")
        );
        if ended {
            self.calls.remove(&id);
        }
        *update = Value::Object(whole);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn merges_incremental_tool_call_updates_and_permission_requests() {
        let mut calls = ToolCalls::default();
        let mut start = json!({
            "method": "session/update",
            "params": {
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "call-1",
                    "title": "Read file",
                    "rawInput": { "path": "/repo/src/lib.rs" }
                }
            }
        });
        calls.merge(&mut start);

        let mut perm = json!({
            "method": "session/request_permission",
            "params": { "toolCall": { "toolCallId": "call-1" } }
        });
        calls.merge(&mut perm);
        assert_eq!(perm["params"]["toolCall"]["title"], "Read file");
    }
}
