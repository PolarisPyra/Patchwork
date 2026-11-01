//! Keeping the conversation inside the model's context: cutting old tool results and images when a request is too long, and summarizing it.

use serde_json::{Value, json};
use super::{Agent, Failure, Reply, emit};

/// Tool results kept whole each time a conversation is cut to fit: older ones become a note.
const KEEP_RESULTS: [usize; 3] = [8, 2, 0];

const LEFT_OUT: &str = "(Earlier messages were left out to fit the context window.)";

/// Phrases of the errors endpoints answer a too-long request with: OpenAI, DeepSeek, vLLM, and
/// xAI ("maximum context/prompt length"), Groq (`context_length_exceeded`, "reduce the length"),
/// Mistral and llama.cpp ("context size", "maximum context"), Fireworks ("prompt is too long"),
/// LM Studio ("context the overflows"), TGI-based hosts ("`max_new_tokens` must be"), and
/// OpenRouter, which can wrap them in `metadata.raw`.
pub(super) const CONTEXT_PHRASES: [&str; 9] = [
    "max_new_tokens",
    "context length",
    "context_length",
    "context size",
    "context window",
    "maximum context",
    "prompt length",
    "reduce the length",
    "too long",
];

const REMOVED_RESULT: &str = "(This output was removed to fit the context window. Run the tool again if it is still needed.)";

/// Replaces the images in a message with a note that they were left out; returns how many.
fn strip_images(message: &mut Value, provider: &str) -> usize {
    let Some(parts) = message["content"].as_array() else {
        return 0;
    };
    let images = parts.iter().filter(|p| p["type"] == "image_url").count();
    if images > 0 {
        let text = parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        message["content"] = json!(format!("{text}\n\n{}", images_note(provider, images)));
    }
    images
}

/// Stands in for images a provider cannot read, so the model can tell the user.
fn images_note(provider: &str, count: usize) -> String {
    let what = if count == 1 {
        "an image, which"
    } else {
        "images, which"
    };
    let it = if count == 1 { "it was" } else { "they were" };
    format!("(The user attached {what} {provider} cannot read, so {it} left out. Tell the user.)")
}

/// Characters per token, for sizing text the endpoint has not counted.
const CHARS_PER_TOKEN: u64 = 4;

/// The share of the model's context window a conversation fills before it is compacted, leaving
/// room for the reply and the next tool output.
const MODEL_SHARE: (u64, u64) = (4, 5);

/// The characters of a message the model reads as text: its content, without image data, and the
/// arguments of the calls it makes.
fn text_len(message: &Value) -> usize {
    let content = match &message["content"] {
        Value::String(text) => text.len(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .map(str::len)
            .sum(),
        _ => 0,
    };
    let calls: usize = message["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["function"]["arguments"].as_str())
        .map(str::len)
        .sum();
    content + calls
}

const COMPACT_PROMPT: &str = "\
Summarize the conversation below, between a user and a coding agent, so that the agent can continue \
the work from the summary alone. Include the user's goals and instructions, decisions made, files \
read or changed with what was learned or done, commands run and their results, the full task list \
(update_plan steps), each with its status, and what is left to do. Do not continue the work \
yourself. Reply with the summary only.";

/// The conversation as one transcript, quoted in a request to summarize it. The request is a message
/// of its own, sent without tools, so the model summarizes the transcript instead of carrying on.
fn summary_request(messages: &[Value]) -> Value {
    let transcript = messages
        .iter()
        .map(|m| {
            let mut text = match &m["content"] {
                Value::String(text) => text.clone(),
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(|p| match p["type"].as_str() {
                        Some("image_url") => Some("[image]"),
                        _ => p["text"].as_str(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            };
            for call in m["tool_calls"].as_array().into_iter().flatten() {
                let function = &call["function"];
                text.push_str(&format!(
                    "\n[called {} with {}]",
                    function["name"].as_str().unwrap_or_default(),
                    function["arguments"].as_str().unwrap_or_default()
                ));
            }
            let who = match m["role"].as_str() {
                Some("assistant") => "Agent",
                Some("tool") => "Tool result",
                _ => "User",
            };
            format!("{who}: {}", text.trim())
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    json!({"role": "user", "content": format!("{COMPACT_PROMPT}\n\n<conversation>\n{transcript}\n</conversation>")})
}

impl Agent {
    /// Summarizes the conversation and keeps only the summary.
    pub(super) fn compact(&mut self) -> Result<(), String> {
        if self.messages.is_empty() {
            emit(&json!({"type": "api.text", "delta": "There is nothing to compact yet."}));
            return Ok(());
        }
        self.summarize(false)?;
        self.save()
    }

    /// Summarizes the conversation before the next request once it reached the size to compact
    /// at, so the request starts small again.
    pub(super) fn compact_if_full(&mut self) -> Result<(), String> {
        let at = self.compact_at();
        // When the system prompt and tools alone fill that much, summarizing frees nothing.
        if at == 0 || at <= self.fixed_size() || self.context.max(self.estimate()) < at {
            return Ok(());
        }
        emit(&json!({"type": "api.compacting"}));
        self.summarize(true)?;
        self.save()
    }

    /// The size to compact at: the chat's compact window, or less when the model's own context
    /// window is smaller, so a local model loaded with a small context compacts before it is full.
    fn compact_at(&mut self) -> u64 {
        let model = self
            .model_window()
            .map_or(0, |w| w * MODEL_SHARE.0 / MODEL_SHARE.1);
        match (self.request.compact_window, model) {
            (0, model) => model,
            (window, 0) => window,
            (window, model) => window.min(model),
        }
    }

    /// The model's context window, asked of the endpoint once a turn.
    fn model_window(&mut self) -> Option<u64> {
        if self.model_window.is_none() {
            self.model_window = Some(self.endpoint_window());
        }
        self.model_window.flatten()
    }

    /// The size the system prompt and tools take in every request, estimated from their text.
    fn fixed_size(&self) -> u64 {
        (self.system.len() + self.tools.to_string().len()) as u64 / CHARS_PER_TOKEN
    }

    /// The size of the next request, estimated from its text. The endpoint's own count comes only
    /// with a reply, so it leaves out the tool output since then, and LM Studio counts a prompt
    /// only after cutting it to fit.
    fn estimate(&self) -> u64 {
        let text: usize = self.messages.iter().map(text_len).sum();
        self.fixed_size() + text as u64 / CHARS_PER_TOKEN
    }

    /// Replaces the conversation with the model's summary of it. With `resume`, the user's latest
    /// message follows the summary word for word, as the request to answer; without it, the
    /// summary is the reply.
    pub(super) fn summarize(&mut self, resume: bool) -> Result<(), String> {
        let latest = resume
            .then(|| self.messages.iter().rev().find(|m| m["role"] == "user"))
            .flatten()
            .and_then(|m| m["content"].as_str())
            .map(str::to_string);
        // A retry after a context error may follow part of a reply, so the summary does not stream.
        self.quiet = true;
        let reply = self.summarize_transcript();
        self.quiet = false;
        let reply = reply?;
        if reply.content.trim().is_empty() {
            return Err("The model returned an empty summary, so the conversation was kept".into());
        }
        if resume {
            emit(&json!({"type": "api.compacted", "summary": reply.content.trim()}));
        } else {
            emit(&json!({"type": "api.text", "delta": reply.content}));
        }
        let mut summary = format!(
            "This conversation was compacted. A summary of it so far:\n\n{}",
            reply.content.trim()
        );
        if let Some(latest) = latest {
            summary.push_str(&format!(
                "\n\nThe user's latest message, which the work above was answering:\n\n{latest}"
            ));
        }
        self.messages = vec![json!({"role": "user", "content": summary})];
        self.context = 0;
        Ok(())
    }

    /// The model's summary of the conversation, quoted as one transcript. When it does not fit the
    /// model's window, the oldest messages are left out of it until it does.
    fn summarize_transcript(&mut self) -> Result<Reply, Failure> {
        // An endpoint that cuts a prompt to fit, as LM Studio does, would summarize only part of
        // the conversation, so when the model's window is known the oldest messages go first.
        if let Some(window) = self.model_window() {
            while self.estimate() > window * 9 / 10 && self.drop_oldest() {}
        }
        loop {
            let request = summary_request(&self.messages);
            let history = std::mem::replace(&mut self.messages, vec![request]);
            let reply = self.complete(false);
            self.messages = history;
            match reply {
                Err(Failure::Context(_)) if self.drop_oldest() => continue,
                reply => return reply,
            }
        }
    }

    /// Retries a request the provider refused for the conversation's size or its images, after
    /// making it fit: images out, then old tool output cut, then the whole thing summarized.
    pub(super) fn recover(&mut self, mut failure: Failure) -> Result<Reply, String> {
        let (mut imageless, mut stage, mut summarized) = (false, 0, false);
        loop {
            match failure {
                Failure::Images(_) if !imageless && self.drop_images() => imageless = true,
                Failure::Context(_) if stage < KEEP_RESULTS.len() => {
                    // A stage that cut nothing is skipped without a request.
                    let cut = self.trim_results(KEEP_RESULTS[stage]);
                    stage += 1;
                    if !cut {
                        continue;
                    }
                }
                Failure::Context(_) if !summarized => {
                    emit(&json!({"type": "api.compacting"}));
                    self.summarize(true)?;
                    summarized = true;
                }
                failure => return Err(failure.into()),
            }
            self.save()?;
            match self.complete(true) {
                Ok(reply) => return Ok(reply),
                Err(next) => failure = next,
            }
        }
    }

    /// Cuts every tool result but the last `keep` to a note; returns whether any was cut.
    pub(super) fn trim_results(&mut self, keep: usize) -> bool {
        let results: Vec<usize> = (0..self.messages.len())
            .filter(|&i| self.messages[i]["role"] == "tool")
            .collect();
        let mut cut = false;
        for &i in results.iter().rev().skip(keep) {
            let content = &mut self.messages[i]["content"];
            if content
                .as_str()
                .is_some_and(|c| c.len() > REMOVED_RESULT.len())
            {
                *content = json!(REMOVED_RESULT);
                cut = true;
            }
        }
        cut
    }

    /// Leaves out the older half of the conversation, starting it again at a user message where
    /// one follows, and never at a tool result whose call was left out; returns whether any was.
    pub(super) fn drop_oldest(&mut self) -> bool {
        let len = self.messages.len();
        if len < 2 {
            return false;
        }
        let half = len / 2;
        let at = |role: &str| (half..len).find(|&i| self.messages[i]["role"] == role);
        let start = at("user")
            .or_else(|| (half..len).find(|&i| self.messages[i]["role"] != "tool"))
            .unwrap_or(len - 1);
        self.messages.drain(..start);
        if self.messages.first().is_some_and(|m| m["role"] != "user") {
            self.messages
                .insert(0, json!({"role": "user", "content": LEFT_OUT}));
        }
        true
    }

    /// Takes the images out of the conversation, noting where they were; returns whether any
    /// were there.
    pub(super) fn drop_images(&mut self) -> bool {
        let mut dropped = false;
        let provider = &self.request.provider;
        for m in &mut self.messages {
            dropped |= strip_images(m, provider) > 0;
        }
        dropped
    }

    /// The conversation as a model that cannot read images gets it: the images left out.
    pub(super) fn without_images(&self) -> Vec<Value> {
        let mut messages = self.messages.clone();
        for m in &mut messages {
            strip_images(m, &self.request.provider);
        }
        messages
    }
}
