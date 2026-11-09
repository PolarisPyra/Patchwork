//! Patchwork's own coding-agent harness for chat models behind OpenAI-compatible endpoints.
//!
//! Patchwork's own agent harness: what turns a chat model behind an OpenAI-compatible endpoint
//! into a coding agent. `agent` runs the loop (the request, the reply's tool calls, and keeping
//! the conversation inside the model's context), and `tools` is what the model can do. Providers
//! that bring their own harness, such as Claude Code or Codex, do not use it; `provider::api`
//! starts it for API providers and maps the lines it writes.

// Rust guideline compliant 2026-10-09

pub(crate) mod agent;
pub mod tools;

#[doc(inline)]
pub use agent::{BRIDGE_ARG, error_text, run};
