//! Names a thread after what its first message is about, with a one-off call to a small model.

use std::time::Duration;

use serde_json::{Value, json};

use crate::provider;
use crate::types::AgentKind;

/// A title call that has not answered by then is given up, and the thread keeps its first line.
const TIMEOUT: Duration = Duration::from_secs(60);

/// The longest part of the message the model reads.
const MAX_MESSAGE_CHARS: usize = 8_000;

const SCHEMA: &str = r#"{"type":"object","properties":{"title":{"type":"string"}},"required":["title"],"additionalProperties":false}"#;

const PROMPT: &str = "Generate a title that will help the user recognize this coding thread weeks later.
Return JSON with the key title.

Before answering, silently reduce the request to:
- Subject: What system, feature, or problem is this really about?
- Outcome: What does the user ultimately want to understand or change?
- Incidental instructions: What only describes how the agent should do the work?

Title the subject and outcome. Discard incidental instructions.

Editorial rules:
- 3-8 words, fewer than 40 characters.
- Use a compact noun phrase or clear action phrase.
- When the message is mostly code, logs, or an error, name what the code does or what is failing.
- Capture the umbrella goal when the request lists several symptoms or steps.
- Models, subagents, tools, and output formats do not belong in the title unless they are themselves the topic.
- Do not claim the work is complete.
- Do not copy and truncate the user's message.
- Avoid quotes, labels, filler, and trailing punctuation.";

/// A title for a thread that starts with `message`, from the provider the thread runs on:
/// Claude, Codex, or its API provider with `model`. Other agents use Claude, else Codex. None
/// when none answers.
pub fn generate(message: &str, agent: AgentKind, model: Option<&str>) -> Option<String> {
    let message: String = message.chars().take(MAX_MESSAGE_CHARS).collect();
    if agent == AgentKind::Api {
        return from_api(&message, model);
    }
    let prompt = format!("{PROMPT}\n\nUser message:\n{message}");
    // A folder of its own, so the model reads no project instructions.
    let dir = std::env::temp_dir().join(format!("patchwork-title-{}", crate::types::new_id()));
    std::fs::create_dir_all(&dir).ok()?;
    let title = match agent {
        AgentKind::Claude => from_claude(&prompt, &dir),
        AgentKind::Codex => from_codex(&prompt, &dir, model),
        _ => from_claude(&prompt, &dir).or_else(|| from_codex(&prompt, &dir, None)),
    };
    let _ = std::fs::remove_dir_all(&dir);
    title
}

/// Asks the thread's OpenAI-compatible endpoint, such as DeepSeek's, with the thread's model.
fn from_api(message: &str, model: Option<&str>) -> Option<String> {
    let connection = provider::api_config::connection(model).ok()?;
    let base = connection["baseUrl"].as_str()?.trim_end_matches('/');
    let body = json!({
        "model": connection["model"],
        "messages": [
            {"role": "system", "content": PROMPT},
            {"role": "user", "content": message},
        ],
        "stream": false,
    });
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    let mut request = agent
        .post(format!("{base}/chat/completions"))
        .header("Content-Type", "application/json");
    if let Some(key) = connection["apiKey"].as_str() {
        request = request.header("Authorization", &format!("Bearer {key}"));
    }
    for (name, value) in connection["headers"].as_object().into_iter().flatten() {
        request = request.header(name, value.as_str().unwrap_or_default());
    }
    let mut response = request.send(body.to_string()).ok()?;
    let text = response.body_mut().read_to_string().unwrap_or_default();
    if !response.status().is_success() {
        log::warn!("title generation failed: {}", text.trim());
        return None;
    }
    let reply: Value = serde_json::from_str(&text).ok()?;
    let content = reply["choices"][0]["message"]["content"].as_str()?.trim();
    // The JSON asked for, maybe in a code fence; a model that answers in plain text is taken
    // at its word.
    let unfenced = content
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    match serde_json::from_str::<Value>(unfenced) {
        Ok(value) => clean(value["title"].as_str()?),
        Err(_) => clean(content),
    }
}

fn from_claude(prompt: &str, dir: &std::path::Path) -> Option<String> {
    let args = [
        "-p",
        prompt,
        "--output-format",
        "json",
        "--json-schema",
        SCHEMA,
        "--model",
        "haiku",
        "--tools",
        "",
        "--disable-slash-commands",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--settings",
        r#"{"disableAllHooks":true}"#,
        "--permission-mode",
        "dontAsk",
    ];
    let out = provider::probe_in(AgentKind::Claude, &args, dir, TIMEOUT)?;
    if !out.success {
        log::warn!("title generation failed: {}", out.stderr.trim());
        return None;
    }
    let value: Value = serde_json::from_str(&out.stdout).ok()?;
    clean(value["structured_output"]["title"].as_str()?)
}

/// With the thread's model when it has one, at low effort, else Codex's default model.
fn from_codex(prompt: &str, dir: &std::path::Path, model: Option<&str>) -> Option<String> {
    let schema = dir.join("schema.json");
    let answer = dir.join("answer.json");
    std::fs::write(&schema, SCHEMA).ok()?;
    let mut args = vec![
        "exec",
        "--ephemeral",
        "--skip-git-repo-check",
        "--sandbox",
        "read-only",
        "-c",
        "model_reasoning_effort=\"low\"",
        "--output-schema",
        schema.to_str()?,
        "--output-last-message",
        answer.to_str()?,
    ];
    if let Some(model) = model {
        args.extend(["-m", model]);
    }
    args.push(prompt);
    let out = provider::probe_in(AgentKind::Codex, &args, dir, TIMEOUT)?;
    if !out.success {
        log::warn!("title generation failed: {}", out.stderr.trim());
        return None;
    }
    let value: Value = serde_json::from_str(&std::fs::read_to_string(&answer).ok()?).ok()?;
    clean(value["title"].as_str()?)
}

/// One line without wrapping quotes or a trailing period, at most 120 characters.
fn clean(raw: &str) -> Option<String> {
    let line = raw.lines().next().unwrap_or("");
    let title = line
        .trim()
        .trim_matches(|c| matches!(c, '"' | '\'' | '`'))
        .trim_end_matches('.')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!title.is_empty()).then(|| title.chars().take(120).collect())
}
