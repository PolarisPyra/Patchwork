//! Patchwork's own coding-agent harness for chat models behind OpenAI-compatible endpoints.
//!
//! Patchwork's own agent harness: what turns a chat model behind an OpenAI-compatible endpoint
//! into a coding agent. `agent` runs the loop (the request, the reply's tool calls, and keeping
//! the conversation inside the model's context), and `tools` is what the model can do. Providers
//! that bring their own harness, such as Claude Code or Codex, do not use it; `provider::api`
//! starts it for API providers and maps the lines it writes.

//!
//! It also holds the rest of what Patchwork's binary runs as a helper process, which
//! [`helper_role`] picks by its first argument: the permission hook, Antigravity's system prompt
//! hook, and the relay to an MCP server Patchwork runs. `patchwork-remote` runs the same helpers
//! on an SSH host, so this crate links no store, window or sidecar.

// Rust guideline compliant 2026-11-27

pub(crate) mod agent;
pub mod computer;
pub mod hooks;
pub mod images;
pub mod mcp_proxy;
pub mod own_tools;
pub mod permission_hook;
pub mod prompt_hook;
pub mod target;
pub mod tools;

pub use patchwork_platform::Error;

#[doc(inline)]
pub use agent::request::send::error_text;
#[doc(inline)]
pub use agent::turn::{BRIDGE_ARG, run};

/// The helper `arg` starts Patchwork's binary as, run by calling what this returns, whose result
/// is the exit code. None when `arg` names no helper.
#[must_use]
pub fn helper_role(arg: &str) -> Option<fn() -> i32> {
    Some(match arg {
        // An OpenCode turn's bridge to `opencode serve`.
        patchwork_provider_opencode::bridge::BRIDGE_ARG => patchwork_provider_opencode::bridge::run,
        // An API provider's turn, run as Patchwork's own agent.
        BRIDGE_ARG => run,
        // Checks a tool call against the permission rules.
        permission_hook::HOOK_ARG => permission_hook::run,
        // Antigravity's session start, for the system prompt from Settings.
        prompt_hook::PROMPT_ARG => prompt_hook::run_prompt_hook,
        // Claude Code's and Codex's relay to an MCP server Patchwork runs.
        mcp_proxy::PROXY_ARG => mcp_proxy::run_proxy,
        _ => return None,
    })
}
