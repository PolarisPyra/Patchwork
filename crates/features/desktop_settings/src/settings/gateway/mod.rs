//! Settings → Gateway: the chat apps Patchwork answers on. Each has its tokens, kept in the
//! system credential store, its connection's status, and a switch; the pairings made with a
//! one-time code are listed below them.

mod general;
mod paired;
mod platform;
mod layout;
mod setup;
mod connections;

pub use layout::render;
pub use connections::GatewayState;
