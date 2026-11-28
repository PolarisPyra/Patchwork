//! Antigravity's session start hook, which hands it the system prompt from Settings.

// Rust guideline compliant 2026-11-27

use std::io::Write;

use serde_json::json;

/// The argument that starts Patchwork's binary as Antigravity's session start hook, which hands it
/// the system prompt from Settings.
pub const PROMPT_ARG: &str = "--system-prompt-hook";
/// Where that hook finds the prompt: the turn's environment, which Antigravity passes on to it.
pub const PROMPT_ENV: &str = "PATCHWORK_SYSTEM_PROMPT";

/// Runs the session start hook: answers Antigravity with the system prompt as a system message,
/// which it adds to the conversation before the first model call.
#[must_use]
pub fn run_prompt_hook() -> i32 {
    let answer = match std::env::var(PROMPT_ENV) {
        Ok(text) if !text.trim().is_empty() => {
            json!({"injectSteps": [{"systemMessage": {"systemMessage": text}}]})
        }
        _ => json!({}),
    };
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{answer}");
    let _ = out.flush();
    0
}
