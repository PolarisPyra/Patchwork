//! The body of a request: the conversation as the provider reads it, with the cache marks.

// Rust guideline compliant 2026-10-09

use std::borrow::Cow;
use serde::ser::SerializeSeq;
use serde::Serialize;
use serde_json::{Value, json};

/// The request's messages: the system prompt, then the conversation, written straight from the
/// history without copying it. Reasoning goes back only for the current turn, as endpoints that
/// return `reasoning_content` ask; earlier turns drop it.
pub(super) struct Messages<'a> {
    pub(in crate::harness::agent) system: &'a str,
    pub(in crate::harness::agent) history: &'a [Value],
    pub(in crate::harness::agent) last_user: usize,
    /// The cache mark, when the model caches only up to marked points: it goes on the system
    /// prompt and on the latest message with text.
    pub(in crate::harness::agent) mark: Option<&'a Value>,
}

impl Serialize for Messages<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.history.len() + 1))?;
        let system = match self.mark {
            Some(mark) => json!([{"type": "text", "text": self.system, "cache_control": mark}]),
            None => json!(self.system),
        };
        seq.serialize_element(&json!({"role": "system", "content": system}))?;
        let marked = self
            .mark
            .and_then(|_| self.history.iter().rposition(has_text));
        for (i, m) in self.history.iter().enumerate() {
            let strip = i < self.last_user && m.get("reasoning_content").is_some();
            if !strip && Some(i) != marked {
                seq.serialize_element(m)?;
                continue;
            }
            let mut m = m.clone();
            if strip && let Some(obj) = m.as_object_mut() {
                obj.remove("reasoning_content");
            }
            if Some(i) == marked
                && let Some(mark) = self.mark
            {
                mark_text(&mut m, mark);
            }
            seq.serialize_element(&m)?;
        }
        seq.end()
    }
}

/// Whether a message has text a cache mark can go on.
pub(super) fn has_text(message: &Value) -> bool {
    match &message["content"] {
        Value::String(text) => !text.is_empty(),
        Value::Array(parts) => parts.iter().any(|p| p["type"] == "text"),
        _ => false,
    }
}

/// Puts the cache mark on a message's last text, turning plain text into a text part to hold it.
pub(super) fn mark_text(message: &mut Value, mark: &Value) {
    let Some(content) = message.get_mut("content") else {
        return;
    };
    if let Some(text) = content.as_str().filter(|t| !t.is_empty()) {
        *content = json!([{"type": "text", "text": text, "cache_control": mark}]);
    } else if let Some(part) = content
        .as_array_mut()
        .and_then(|parts| parts.iter_mut().rev().find(|p| p["type"] == "text"))
        .and_then(Value::as_object_mut)
    {
        part.insert("cache_control".into(), mark.clone());
    }
}

#[derive(Serialize)]
pub(super) struct Body<'a> {
    pub(in crate::harness::agent) model: &'a str,
    pub(in crate::harness::agent) messages: Messages<'a>,
    pub(in crate::harness::agent) stream: bool,
    pub(in crate::harness::agent) stream_options: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::harness::agent) tools: Option<Cow<'a, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::harness::agent) reasoning_effort: Option<&'a str>,
    /// Routes the conversation's requests to the machine that holds its cached prefix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::harness::agent) prompt_cache_key: Option<&'a str>,
}
