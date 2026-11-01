//! Patchwork's own agent harness: what turns a chat model behind an OpenAI-compatible endpoint
//! into a coding agent. `agent` runs the loop (the request, the reply's tool calls, and keeping
//! the conversation inside the model's context), and `tools` is what the model can do. Providers
//! that bring their own harness, such as Claude Code or Codex, do not use it; `provider::api`
//! starts it for API providers and maps the lines it writes.

pub mod agent;
pub mod tools;

pub use agent::{BRIDGE_ARG, run};

use serde_json::Value;

/// An error a provider answered with, as one readable line.
pub fn error_text(status: u16, body: &Value, raw: &str) -> String {
    let error = &body["error"];
    let message = error["message"]
        .as_str()
        .or_else(|| error.as_str())
        .or_else(|| body["message"].as_str())
        .or_else(|| body["detail"].as_str())
        .map(String::from)
        .unwrap_or_else(|| raw.trim().chars().take(300).collect());
    let hint = match status {
        401 | 403 => " Check the API key in Settings → API Providers.",
        402 => " The account may be out of credit.",
        404 => " Check the base URL and the model id.",
        _ => "",
    };
    match status {
        0 => message,
        _ if message.is_empty() => format!("The provider answered {status}.{hint}"),
        _ => format!("{message} ({status}).{hint}"),
    }
}
