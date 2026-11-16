//! The gateway: chat apps that reach Patchwork's threads. A paired user's messages go to the
//! thread their conversation is bound to, and the agent's requests and replies come back.

mod commands;
mod control;
mod discord;
mod handoff;
mod hub;
mod inbound;
mod matrix;
mod mattermost;
mod net;
mod outbound;
mod platform;
mod qq;
mod records;
mod signal;
mod slack;
mod telegram;
mod types;

pub use hub::Hub;
pub use platform::{Field, Need, Platform};
pub use types::{Binding, ChatApps, ChatPlace, GatewaySettings, PairedUser, PlatformSettings, Status};
