//! The environment that points an agent's permission hook at Patchwork. The hook itself runs in
//! `patchwork_harness::permission_hook`.

// Rust guideline compliant 2026-11-27

use serde_json::json;

use crate::provider::{AgentKind, ProviderKind};
pub use patchwork_harness::permission_hook::{ENV, HOOK_ARG, run};

/// The environment that tells a permission hook which chat it serves.
///
/// Environment that tells the hook which chat it works for, for the agents that run one:
/// Codex, Claude, and Antigravity. The relay's port and token are copied in: a hook that read
/// them from the file could reach another copy of Patchwork that shares the state folder and
/// wrote that file since.
#[must_use]
pub fn env(agent: AgentKind, chat_id: &str, root: &str) -> Vec<(&'static str, String)> {
    if !agent.capabilities().runs_permission_hook {
        return Vec::new();
    }
    let Some(endpoint) = crate::provider::mcp::relay_endpoint() else {
        return Vec::new();
    };
    let context = json!({
        "port": endpoint["port"],
        "token": endpoint["token"],
        "chat": chat_id,
        "root": root,
    });
    vec![(ENV, context.to_string())]
}
