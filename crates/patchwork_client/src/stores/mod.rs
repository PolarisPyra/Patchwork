//! The client state every app reads, desktop and phone: how the connection stands, the chats,
//! the open chat's messages, and the approvals waiting. Each store keeps its own part current
//! from what a [`crate::Transport`] answers and pushes.

// Rust guideline compliant 2026-10-09

mod approvals;
mod chats;
mod connection;
mod messages;

pub use approvals::{ApprovalsStore, PendingApproval};
pub use chats::ChatsStore;
pub use connection::{ConnectionState, Pairing};
pub use messages::MessagesStore;
