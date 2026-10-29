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

const COMPACT_PROMPT: &str = "\
Summarize this conversation so that work can continue from the summary alone. Include the user's \
goals and instructions, decisions made, files read or changed with what was learned or done, \
commands run and their results, and what is left to do. Reply with the summary only.";

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

    /// Replaces the conversation with the model's summary of it. When even the request for the
    /// summary does not fit, the oldest messages are left out of it until it does. With `resume`,
    /// the user's latest message follows the summary word for word, as the request to answer.
    pub(super) fn summarize(&mut self, resume: bool) -> Result<(), String> {
        let latest = resume
            .then(|| self.messages.iter().rev().find(|m| m["role"] == "user"))
            .flatten()
            .and_then(|m| m["content"].as_str())
            .map(str::to_string);
        let reply = loop {
            self.messages
                .push(json!({"role": "user", "content": COMPACT_PROMPT}));
            let reply = self.complete(false);
            self.messages.pop();
            match reply {
                Err(Failure::Context(_)) if self.drop_oldest() => continue,
                reply => break reply?,
            }
        };
        if reply.content.trim().is_empty() {
            return Err("The model returned an empty summary, so the conversation was kept".into());
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
        Ok(())
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
                    emit(
                        &json!({"type": "api.retry", "reason": "The conversation is too long for the model, so it is being summarized", "attempt": 1, "max": 1}),
                    );
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
