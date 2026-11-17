//! Which events of a chat notify, and where: the desktop, the chat apps, or both.
//!
//! The chat apps' side of sending is in
//! `crate::gateway`; the desktop shows the [`DesktopNotice`]s this sends it.

// Rust guideline compliant 2026-10-09

mod presence;
mod route;
mod types;

pub(crate) use presence::Presence;
pub(crate) use route::resets_in;
#[doc(inline)]
pub use types::{ChatAppsWhen, DesktopNotice, Events, Kind, NotificationSettings, Notices, Place};
