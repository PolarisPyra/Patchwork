//! Telling a call of the quilt tool from other tool calls.

// Rust guideline compliant 2026-11-27

use crate::chats::ToolCall;

/// The tool's name on Patchwork's own server.
pub const TOOL: &str = "quilt";

/// The tool's name before it was called `quilt`, which older chats' calls hold.
const OLD_TOOL: &str = "artifact";

/// Whether `tool` is a call of the quilt tool, by its name now or before: by Claude, by Codex,
/// or through Patchwork's own harness, which names it as the tool is named.
#[must_use]
pub fn is_quilt_call(tool: &ToolCall) -> bool {
    [TOOL, OLD_TOOL].into_iter().any(|name| {
        tool.name == name
            || tool.name == format!("mcp__patchwork__{name}")
            || (tool.name == "MCP" && tool.summary == format!("patchwork.{name}"))
    })
}
