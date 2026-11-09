//! Settings → MCP Servers (`providers/mcp/`): Patchwork's own servers and the CLIs', one card
//! per server, opening to its page, and the form that adds or changes one of Patchwork's.

mod actions;
mod detail;
mod form;
mod render;
mod rows;
mod state;

pub use form::Draft;
pub use state::*;
pub use render::*;
