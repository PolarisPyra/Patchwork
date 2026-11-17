//! Watching a reply as it streams: a model stuck repeating itself, its speed, its usage, and how long to wait before a retry.

// Rust guideline compliant 2026-10-09

use super::types::MAX_RETRY_AFTER;
use super::types::MAX_BACKOFF;
use super::types::LOOP_TIMES;
use super::types::LOOP_UNIT;
use super::types::LOOP_STEP;
use std::time::Duration;
use serde_json::{Value, json};
use crate::harness::agent::emit;

/// Spots a reply stuck in a loop: the same 64 to 1024 bytes at least 8 times in a row at its end,
/// found at two checks 1 KB apart, so a list or table that repeats for a moment is left alone.
#[derive(Default)]
pub(super) struct LoopWatch {
    checked: usize,
    hits: u32,
}

impl LoopWatch {
    pub(super) fn looping(&mut self, text: &str) -> bool {
        if text.len() < self.checked + LOOP_STEP {
            return false;
        }
        self.checked = text.len();
        self.hits = if repeats(text.as_bytes()) {
            self.hits + 1
        } else {
            0
        };
        self.hits >= 2
    }
}

/// Whether `text` ends in one stretch repeated `LOOP_TIMES` times: the end shifted by the
/// stretch's length matches itself.
pub(super) fn repeats(text: &[u8]) -> bool {
    LOOP_UNIT.into_iter().any(|unit| {
        let span = unit * LOOP_TIMES;
        if text.len() < span {
            return false;
        }
        let tail = &text[text.len() - span..];
        // The last two stretches first, which rules out nearly every length cheaply.
        tail[span - unit..] == tail[span - 2 * unit..span - unit]
            && tail[unit..] == tail[..span - unit]
    })
}

/// How fast a request answered: the time to its first token, and its output tokens per second
/// from then on.
pub(super) struct Speed {
    pub(super) first_token_ms: u64,
    pub(super) output_per_second: Option<u64>,
}

/// Reports a request's tokens. Cache hits are OpenAI's and OpenRouter's cached-token details,
/// DeepSeek's hit count, or Kimi's top-level count; OpenRouter also counts the tokens a request
/// wrote to the cache. The prompt's count includes both. Returns the tokens of the request and
/// its reply.
pub(super) fn emit_usage(usage: &Value, speed: Option<&Speed>) -> u64 {
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
        "firstTokenMs": speed.map(|s| s.first_token_ms),
        "outputPerSecond": speed.and_then(|s| s.output_per_second),
    }));
    prompt + count(&usage["completion_tokens"])
}

/// The seconds a `Retry-After` header asks to wait, given as seconds or as an HTTP date.
pub(super) fn parse_retry_after(value: &str) -> Option<u64> {
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
pub(super) fn retry_delay(attempt: u32, retry_after: Option<u64>) -> Duration {
    let base = retry_after.map_or(2u64.pow(attempt).min(MAX_BACKOFF), |s| {
        s.min(MAX_RETRY_AFTER)
    });
    let jitter = (uuid::Uuid::new_v4().as_u128() % 1000) as u64;
    Duration::from_millis(base * 1000 + jitter)
}
