//! The gateway's side in the core: chat apps that reach Patchwork's chats.
//!
//! The core keeps their settings, pairings, bindings and personalities, and opens the chats they
//! run; the connections themselves run in `patchwork_gateway`, which the core calls through
//! [`ChatAppHooks`].

mod credentials;
mod files;
mod hooks;
pub mod options;
mod personalities;
mod platform;
mod records;
mod runs;
mod types;
mod update;

pub use credentials::{secret_name, token};
pub use files::{FILE_LIMIT, SCREENSHOTS_OFF, is_picture, mime};
pub use hooks::ChatAppHooks;
pub use personalities::Personality;
pub use platform::{Field, Need, Platform};
pub use types::{
    Binding, ChatAppCall, ChatApps, ChatPlace, GatewaySettings, Pairing, Place, PlatformSettings,
    Presence, Status,
};
pub use update::{AFTER_UPDATE, AfterUpdate};
