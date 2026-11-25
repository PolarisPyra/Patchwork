//! The gateway: chat apps that reach Patchwork's chats. A pairing's messages go to the
//! chat their conversation is bound to, and the agent's requests and replies come back.

mod commands;
mod control;
mod discord;
mod files;
mod handoff;
mod hub;
mod inbound;
mod matrix;
mod mattermost;
mod net;
mod notify;
pub(crate) mod options;
mod outbound;
mod personalities;
mod platform;
mod qq;
mod records;
mod signal;
mod slack;
mod telegram;
mod types;
mod watch;

pub(crate) use files::FILE_LIMIT;
pub use hub::Hub;
pub(crate) use net::is_picture;
pub use personalities::Personality;
pub use platform::{Field, Need, Platform};
pub use types::{
    Binding, ChatAppCall, ChatApps, ChatPlace, GatewaySettings, Pairing, PlatformSettings,
    Presence, Status,
};
