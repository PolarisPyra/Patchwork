//! Talking to the endpoint: the request body, retries, and reading the streamed reply.

use std::borrow::Cow;
use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;
use serde::ser::SerializeSeq;
use serde::Serialize;
use serde_json::{Value, json};
use super::context::CONTEXT_PHRASES;
use super::{Agent, emit};

/// Attempts at a request that failed for a reason worth retrying. A busy endpoint, as DeepSeek's
/// is at peak hours, can take most of a minute to answer again.
const MAX_ATTEMPTS: u32 = 6;

/// The longest wait a provider's `Retry-After` gets before a retry.
const MAX_RETRY_AFTER: u64 = 60;

/// The longest wait between attempts when the provider does not say how long.
const MAX_BACKOFF: u64 = 30;

/// Why a streamed reply failed, and whether any of it was shown, which rules out sending the
/// request again.
pub(super) struct StreamError {
    message: String,
    shown: bool,
}

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
    /// The cache mark, when the model caches only up to marked points: it goes on the system
    /// prompt and on the latest message with text.
    pub(super) mark: Option<&'a Value>,
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
fn has_text(message: &Value) -> bool {
    match &message["content"] {
        Value::String(text) => !text.is_empty(),
        Value::Array(parts) => parts.iter().any(|p| p["type"] == "text"),
        _ => false,
    }
}

/// Puts the cache mark on a message's last text, turning plain text into a text part to hold it.
fn mark_text(message: &mut Value, mark: &Value) {
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
struct Body<'a> {
    pub(super) model: &'a str,
    pub(super) messages: Messages<'a>,
    pub(super) stream: bool,
    pub(super) stream_options: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tools: Option<Cow<'a, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reasoning_effort: Option<&'a str>,
    /// Routes the conversation's requests to the machine that holds its cached prefix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) prompt_cache_key: Option<&'a str>,
}

/// One reply of the model.
#[derive(Default)]
pub(super) struct Reply {
    pub(super) content: String,
    pub(super) reasoning: String,
    /// The endpoint sent its reasoning as `reasoning_content`, which it wants back.
    pub(super) reasoning_content: bool,
    pub(super) calls: Vec<Call>,
    /// Tokens of the request and the reply, which the next request starts from; 0 when the
    /// endpoint reported none.
    pub(super) context: u64,
    /// The reply stopped at the model's output limit, so its text or its last call is cut off.
    pub(super) cut_off: bool,
}

impl Reply {
    /// Nothing came: no text, reasoning, or calls, as an endpoint under load sometimes answers.
    fn is_empty(&self) -> bool {
        self.content.trim().is_empty() && self.reasoning.is_empty() && self.calls.is_empty()
    }
}

#[derive(Default, Clone)]
pub(super) struct Call {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) arguments: String,
}

/// Reports a request's tokens. Cache hits are OpenAI's and OpenRouter's cached-token details,
/// DeepSeek's hit count, or Kimi's top-level count; OpenRouter also counts the tokens a request
/// wrote to the cache. The prompt's count includes both. Returns the tokens of the request and
/// its reply.
fn emit_usage(usage: &Value) -> u64 {
    let count = |v: &Value| v.as_u64().unwrap_or(0);
    let prompt = count(&usage["prompt_tokens"]);
    let cached = usage["prompt_tokens_details"]["cached_tokens"]
        .as_u64()
        .or_else(|| usage["prompt_cache_hit_tokens"].as_u64())
        .or_else(|| usage["cached_tokens"].as_u64())
        .unwrap_or(0);
    let written = count(&usage["prompt_tokens_details"]["cache_write_tokens"]);
    emit(&json!({
        "type": "api.usage",
        "id": uuid::Uuid::new_v4().to_string(),
        "input": prompt.saturating_sub(cached + written),
        "cacheRead": cached,
        "cacheWrite": written,
        "output": count(&usage["completion_tokens"]),
        "context": prompt,
    }));
    prompt + count(&usage["completion_tokens"])
}

/// The seconds a `Retry-After` header asks to wait, given as seconds or as an HTTP date.
fn parse_retry_after(value: &str) -> Option<u64> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds);
    }
    let at = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let seconds = (at.timestamp_millis() - crate::system::now_ms() as i64) / 1000;
    Some(seconds.max(0) as u64)
}

/// How long to wait before attempt `attempt + 1`: what the provider asked for in `Retry-After`, up
/// to a minute, or else 2, 4, 8, 16, then 30 s. Up to a second more at random keeps tasks that
/// failed together from retrying together.
fn retry_delay(attempt: u32, retry_after: Option<u64>) -> Duration {
    let base = retry_after
        .map(|s| s.min(MAX_RETRY_AFTER))
        .unwrap_or(2u64.pow(attempt).min(MAX_BACKOFF));
    let jitter = (uuid::Uuid::new_v4().as_u128() % 1000) as u64;
    Duration::from_millis(base * 1000 + jitter)
}

impl Agent {
    /// Whether the endpoint is on `host`, such as `openrouter.ai`.
    fn on_host(&self, host: &str) -> bool {
        self.request.base_url.to_lowercase().contains(host)
    }

    /// The mark a model that caches only up to marked points needs, as Claude models through
    /// OpenRouter do; None for the rest, which cache the longest prefix a request shares with
    /// earlier ones on their own. Requests here only append to the conversation, so that prefix
    /// is everything up to the latest message.
    fn cache_mark(&self) -> Option<Value> {
        (self.on_host("openrouter.ai") && self.request.model.starts_with("anthropic/"))
            .then(|| json!({"type": "ephemeral"}))
    }

    /// The model's context window as the endpoint reports it: the size LM Studio loaded the model
    /// with, which can be far below its maximum, or what `/models` lists for it (vLLM's
    /// `max_model_len`, OpenRouter's `context_length`, Groq's `context_window`). None when the
    /// endpoint does not say.
    pub(super) fn endpoint_window(&self) -> Option<u64> {
        let base = self.request.base_url.trim_end_matches('/');
        let get = |url: &str| -> Option<Value> {
            let mut request = self
                .http
                .get(url)
                .config()
                .timeout_global(Some(Duration::from_secs(5)))
                .build();
            if let Some(key) = &self.request.api_key {
                request = request.header("Authorization", &format!("Bearer {key}"));
            }
            for (name, value) in &self.request.headers {
                request = request.header(name, value);
            }
            let mut response = request.call().ok()?;
            if !response.status().is_success() {
                return None;
            }
            response.body_mut().read_json().ok()
        };
        let model = self.request.model.as_str();
        if let Some(origin) = base.strip_suffix("/v1")
            && let Some(loaded) = get(&format!("{origin}/api/v0/models/{model}"))
                .and_then(|v| v["loaded_context_length"].as_u64())
        {
            return Some(loaded);
        }
        let listed = get(&format!("{base}/models"))?;
        let entry = listed["data"]
            .as_array()?
            .iter()
            .find(|m| m["id"] == model)?;
        ["max_model_len", "context_length", "context_window"]
            .iter()
            .find_map(|key| entry[*key].as_u64())
    }

    /// Sends the conversation and streams the reply, retrying a request that failed before
    /// any of it arrived.
    pub(super) fn complete(&self, with_tools: bool) -> Result<Reply, Failure> {
        self.send(with_tools.then_some(&self.tools), None)
    }

    /// Sends the conversation with `tools`. Reasoning goes back from `last_user` on, the last
    /// user message by default.
    pub(super) fn send(
        &self,
        tools: Option<&Value>,
        last_user: Option<usize>,
    ) -> Result<Reply, Failure> {
        let history: Cow<[Value]> = if self.request.images_ok {
            Cow::Borrowed(&self.messages[..])
        } else {
            Cow::Owned(self.without_images())
        };
        let last_user = last_user.unwrap_or_else(|| {
            self.messages
                .iter()
                .rposition(|m| m["role"] == "user")
                .unwrap_or(0)
        });
        let mark = self.cache_mark();
        let tools = tools.map(|tools| match &mark {
            Some(mark) => {
                let mut tools = tools.clone();
                if let Some(last) = tools
                    .as_array_mut()
                    .and_then(|list| list.last_mut())
                    .and_then(Value::as_object_mut)
                {
                    last.insert("cache_control".into(), mark.clone());
                }
                Cow::Owned(tools)
            }
            None => Cow::Borrowed(tools),
        });
        let body = Body {
            model: &self.request.model,
            messages: Messages {
                system: &self.system,
                history: &history,
                last_user,
                mark: mark.as_ref(),
            },
            stream: true,
            stream_options: json!({"include_usage": true}),
            tools,
            reasoning_effort: self.request.effort.as_deref(),
            prompt_cache_key: self
                .on_host("api.openai.com")
                .then_some(self.session_id.as_str()),
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
            // OpenRouter routes a session's requests to the provider that holds its cache.
            if self.on_host("openrouter.ai") {
                request = request.header("x-session-id", &self.session_id);
            }
            for (name, value) in &self.request.headers {
                request = request.header(name, value);
            }
            let mut retry_after = None;
            let failure = match request.send(body.as_str()) {
                Ok(response) if response.status().is_success() => {
                    match self.stream(response.into_body().into_reader()) {
                        Ok(reply) if reply.is_empty() && attempt < MAX_ATTEMPTS => (
                            format!("{} sent an empty reply", self.request.provider),
                            true,
                        ),
                        Ok(reply) => return Ok(reply),
                        Err(StreamError { message, shown }) => {
                            // Some endpoints, such as LM Studio, accept the request and then
                            // refuse a prompt too long for the loaded context as an error event
                            // in the stream.
                            let lower = message.to_lowercase();
                            if CONTEXT_PHRASES.iter().any(|p| lower.contains(p)) {
                                return Err(Failure::Context(message));
                            }
                            // A reply that broke off once the user saw part of it is not sent
                            // again, which would show that part twice.
                            (message, !shown)
                        }
                    }
                }
                Ok(mut response) => {
                    retry_after = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(parse_retry_after);
                    let status = response.status().as_u16();
                    let text = response.body_mut().read_to_string().unwrap_or_default();
                    let parsed = serde_json::from_str(&text).unwrap_or(Value::Null);
                    let message = crate::harness::error_text(status, &parsed, &text);
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
            std::thread::sleep(retry_delay(attempt, retry_after));
            attempt += 1;
        }
    }

    /// Reads a server-sent event stream of completion chunks.
    pub(super) fn stream(&self, body: impl Read + Send + 'static) -> Result<Reply, StreamError> {
        let mut reply = Reply::default();
        match self.read_stream(body, &mut reply) {
            Ok(()) => Ok(reply),
            Err(message) => Err(StreamError {
                message,
                // A quiet reply, like a summary, shows nothing until it is done.
                shown: !self.quiet && (!reply.content.is_empty() || !reply.reasoning.is_empty()),
            }),
        }
    }

    /// Reads the chunks into `reply`, which keeps what arrived when it fails.
    fn read_stream(
        &self,
        body: impl Read + Send + 'static,
        reply: &mut Reply,
    ) -> Result<(), String> {
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
                return Err(crate::harness::error_text(0, &chunk, data));
            }
            if chunk["usage"].is_object() {
                usage = chunk["usage"].clone();
            }
            let Some(choice) = chunk["choices"].get(0) else {
                continue;
            };
            if choice["finish_reason"] == "length" {
                reply.cut_off = true;
            }
            let delta = &choice["delta"];
            if let Some(text) = delta["reasoning_content"]
                .as_str()
                .filter(|t| !t.is_empty())
            {
                reply.reasoning_content = true;
                reply.reasoning.push_str(text);
                if !self.quiet {
                    emit(&json!({"type": "api.reasoning", "delta": text}));
                }
            } else if let Some(text) = delta["reasoning"].as_str().filter(|t| !t.is_empty()) {
                reply.reasoning.push_str(text);
                if !self.quiet {
                    emit(&json!({"type": "api.reasoning", "delta": text}));
                }
            }
            if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
                reply.content.push_str(text);
                if !self.quiet {
                    emit(&json!({"type": "api.text", "delta": text}));
                }
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
            reply.context = emit_usage(&usage);
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
        Ok(())
    }
}
