//! Settings → MCP Servers (`providers/mcp/`): Patchwork's own servers and the CLIs', one card
//! per server, opening to its page, and the form that adds or changes one of Patchwork's.

// Rust guideline compliant 2026-10-09

mod actions;
mod detail;
mod form;
mod render;
mod rows;
mod state;

pub use form::Draft;
use state::{Card, Client, Own, Use, dot, target, tildify};
pub use state::McpState;
pub use render::render;
use render::pairs_text;
