//! The gateway: chat apps that reach Patchwork's threads. A pairing's messages go to the
//! thread their conversation is bound to, and the agent's requests and replies come back.

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

pub use hub::Hub;
pub use personalities::Personality;
pub use platform::{Field, Need, Platform};
pub use types::{
    Binding, ChatAppCall, ChatApps, ChatPlace, GatewaySettings, Pairing, PlatformSettings,
    Presence, Status,
};
