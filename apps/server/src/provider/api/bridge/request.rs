//! Talking to the endpoint: the request body, retries, and reading the streamed reply.

use std::borrow::Cow;
use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;
use serde::ser::SerializeSeq;
use serde::Serialize;
use serde_json::{Value, json};
use super::{Agent, CONTEXT_PHRASES, emit};

/// Attempts at a request that failed for a reason worth retrying.
const MAX_ATTEMPTS: u32 = 4;

/// The longest gap between two lines of a streamed reply before it counts as stalled. Endpoints
/// that queue or think send keep-alive comments, which count as lines.
const STREAM_IDLE: Duration = Duration::from_secs(180);

/// Why a request failed: some failures are fixed by making the conversation smaller.
pub(super) enum Failure {
    /// The conversation is longer than the model's context window.
    Context(String),
    /// The model or endpoint does not accept images.
    Images(String),
    Other(String),
}

impl From<Failure> for String {
    fn from(failure: Failure) -> String {
        match failure {
            Failure::Context(m) | Failure::Images(m) | Failure::Other(m) => m,
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Failure {
        Failure::Other(message)
    }
}

/// The request's messages: the system prompt, then the conversation, written straight from the
/// history without copying it. Reasoning goes back only for the current turn, as endpoints that
/// return `reasoning_content` ask; earlier turns drop it.
struct Messages<'a> {
    pub(super) system: &'a str,
    pub(super) history: &'a [Value],
    pub(super) last_user: usize,
}

impl Serialize for Messages<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.history.len() + 1))?;
        seq.serialize_element(&json!({"role": "system", "content": self.system}))?;
        for (i, m) in self.history.iter().enumerate() {
            if i < self.last_user && m.get("reasoning_content").is_some() {
                let mut m = m.clone();
                if let Some(obj) = m.as_object_mut() {
                    obj.remove("reasoning_content");
                }
                seq.serialize_element(&m)?;
            } else {
                seq.serialize_element(m)?;
            }
        }
        seq.end()
    }
}

#[derive(Serialize)]
struct Body<'a> {
    pub(super) model: &'a str,
    pub(super) messages: Messages<'a>,
    pub(super) stream: bool,
    pub(super) stream_options: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tools: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reasoning_effort: Option<&'a str>,
}

/// One reply of the model.
#[derive(Default)]
pub(super) struct Reply {
    pub(super) content: String,
    pub(super) reasoning: String,
    /// The endpoint sent its reasoning as `reasoning_content`, which it wants back.
    pub(super) reasoning_content: bool,
    pub(super) calls: Vec<Call>,
}

#[derive(Default, Clone)]
pub(super) struct Call {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) arguments: String,
}

/// Reports a request's tokens: OpenAI's cached-token details, or DeepSeek's cache hit count.
fn emit_usage(usage: &Value) {
    let count = |v: &Value| v.as_u64().unwrap_or(0);
    let prompt = count(&usage["prompt_tokens"]);
    let cached = usage["prompt_tokens_details"]["cached_tokens"]
        .as_u64()
        .or_else(|| usage["prompt_cache_hit_tokens"].as_u64())
        .unwrap_or(0);
    emit(&json!({
        "type": "api.usage",
        "id": uuid::Uuid::new_v4().to_string(),
        "input": prompt.saturating_sub(cached),
        "cacheRead": cached,
        "output": count(&usage["completion_tokens"]),
        "context": prompt,
    }));
}

impl Agent {
    /// Sends the conversation and streams the reply, retrying a request that failed before
    /// any of it arrived.
    pub(super) fn complete(&self, with_tools: bool) -> Result<Reply, Failure> {
        let history: Cow<[Value]> = if self.request.images_ok {
            Cow::Borrowed(&self.messages[..])
        } else {
            Cow::Owned(self.without_images())
        };
        let body = Body {
            model: &self.request.model,
            messages: Messages {
                system: &self.system,
                history: &history,
                last_user: self
                    .messages
                    .iter()
                    .rposition(|m| m["role"] == "user")
                    .unwrap_or(0),
            },
            stream: true,
            stream_options: json!({"include_usage": true}),
            tools: with_tools.then_some(&self.tools),
            reasoning_effort: self.request.effort.as_deref(),
        };
        let body = serde_json::to_string(&body).map_err(|e| e.to_string())?;
        let has_images = self.messages.iter().any(|m| {
            m["content"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|p| p["type"] == "image_url"))
        });
        let url = format!(
            "{}/chat/completions",
            self.request.base_url.trim_end_matches('/')
        );
        let mut attempt = 1;
        loop {
            let mut request = self
                .http
                .post(&url)
                .header("Content-Type", "application/json");
            if let Some(key) = &self.request.api_key {
                request = request.header("Authorization", &format!("Bearer {key}"));
            }
            for (name, value) in &self.request.headers {
                request = request.header(name, value);
            }
            let failure = match request.send(body.as_str()) {
                Ok(response) if response.status().is_success() => {
                    return self
                        .stream(response.into_body().into_reader())
                        .map_err(Failure::Other);
                }
                Ok(mut response) => {
                    let status = response.status().as_u16();
                    let text = response.body_mut().read_to_string().unwrap_or_default();
                    let parsed = serde_json::from_str(&text).unwrap_or(Value::Null);
                    let message = crate::provider::api::error_text(status, &parsed, &text);
                    // The whole body, so codes and errors wrapped by a router count too.
                    let lower = text.to_lowercase();
                    let refused = matches!(status, 400 | 413 | 415 | 422);
                    if status == 413
                        || (refused && CONTEXT_PHRASES.iter().any(|p| lower.contains(p)))
                    {
                        return Err(Failure::Context(message));
                    }
                    let about_images = lower.contains("image") || lower.contains("multimodal");
                    if refused && has_images && about_images {
                        return Err(Failure::Images(message));
                    }
                    let retry =
                        matches!(status, 408 | 409 | 425 | 429 | 500 | 502 | 503 | 504 | 529);
                    (message, retry)
                }
                Err(e) => (
                    format!("Could not reach {}: {e}", self.request.provider),
                    true,
                ),
            };
            let (message, retry) = failure;
            if !retry || attempt >= MAX_ATTEMPTS {
                return Err(Failure::Other(message));
            }
            emit(
                &json!({"type": "api.retry", "reason": message, "attempt": attempt, "max": MAX_ATTEMPTS - 1}),
            );
            std::thread::sleep(Duration::from_secs(2u64.pow(attempt)));
            attempt += 1;
        }
    }

    /// Reads a server-sent event stream of completion chunks.
    pub(super) fn stream(&self, body: impl Read + Send + 'static) -> Result<Reply, String> {
        let mut reply = Reply::default();
        let mut calls: Vec<Call> = Vec::new();
        let mut usage = Value::Null;
        // Lines come through a thread so a stalled reply can be given up on.
        let (tx, lines) = mpsc::sync_channel(256);
        std::thread::spawn(move || {
            for line in BufReader::new(body).lines() {
                let end = line.is_err();
                if tx.send(line).is_err() || end {
                    break;
                }
            }
        });
        loop {
            let line = match lines.recv_timeout(STREAM_IDLE) {
                Ok(line) => line,
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!(
                        "{} stopped sending its reply for {} s, so the turn was stopped",
                        self.request.provider,
                        STREAM_IDLE.as_secs()
                    ));
                }
            };
            let line = line
                .map_err(|e| format!("The reply from {} broke off: {e}", self.request.provider))?;
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            let Ok(chunk) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if !chunk["error"].is_null() {
                return Err(crate::provider::api::error_text(0, &chunk, data));
            }
            if chunk["usage"].is_object() {
                usage = chunk["usage"].clone();
            }
            let Some(choice) = chunk["choices"].get(0) else {
                continue;
            };
            let delta = &choice["delta"];
            if let Some(text) = delta["reasoning_content"]
                .as_str()
                .filter(|t| !t.is_empty())
            {
                reply.reasoning_content = true;
                reply.reasoning.push_str(text);
                emit(&json!({"type": "api.reasoning", "delta": text}));
            } else if let Some(text) = delta["reasoning"].as_str().filter(|t| !t.is_empty()) {
                reply.reasoning.push_str(text);
                emit(&json!({"type": "api.reasoning", "delta": text}));
            }
            if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
                reply.content.push_str(text);
                emit(&json!({"type": "api.text", "delta": text}));
            }
            for part in delta["tool_calls"].as_array().into_iter().flatten() {
                // Calls carry an index; without one, a new id starts a new call.
                let index = match part["index"].as_u64() {
                    Some(n) => n as usize,
                    None => match part["id"].as_str() {
                        Some(id) if calls.iter().all(|c| c.id != id) => calls.len(),
                        _ => calls.len().saturating_sub(1),
                    },
                };
                if calls.len() <= index {
                    calls.resize(index + 1, Call::default());
                }
                let call = &mut calls[index];
                if let Some(id) = part["id"].as_str().filter(|id| !id.is_empty()) {
                    call.id = id.into();
                }
                if let Some(name) = part["function"]["name"].as_str() {
                    call.name.push_str(name);
                }
                if let Some(arguments) = part["function"]["arguments"].as_str() {
                    call.arguments.push_str(arguments);
                }
            }
        }
        if !usage.is_null() {
            emit_usage(&usage);
        }
        reply.calls = calls
            .into_iter()
            .filter(|c| !c.name.is_empty())
            .map(|mut c| {
                if c.id.is_empty() {
                    c.id = format!("call_{}", uuid::Uuid::new_v4().simple());
                }
                c
            })
            .collect();
        Ok(reply)
    }
}
