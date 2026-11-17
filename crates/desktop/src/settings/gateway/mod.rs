//! Settings → Gateway: the chat apps Patchwork answers on. Each has its tokens, kept in the
//! system credential store, its connection's status, and a switch; the pairings made with a
//! one-time code are listed below them.

mod general;
mod paired;
mod platform;
mod render;
mod setup;
mod state;

pub use render::render;
pub use state::GatewayState;
