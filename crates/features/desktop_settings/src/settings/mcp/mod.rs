//! Settings → MCP Servers (`providers/mcp/`): Patchwork's own servers and the CLIs', one card
//! per server, opening to its page, and the form that adds or changes one of Patchwork's.

// Rust guideline compliant 2026-10-09

mod server_actions;
mod detail;
mod form;
mod layout;
mod rows;
mod servers;

pub use form::Draft;
use servers::{Card, Client, Own, Use, dot, target};
pub use servers::McpState;
pub use layout::render;
use layout::pairs_text;
