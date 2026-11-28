//! What Patchwork's screens read, and the operations they call to change it.
//!
//! The crate draws nothing. It holds the app's data, kept current from a Patchwork, so the
//! desktop's screens and the phone's read the same thing and draw it in their own way.
//!
//! - [`Session`] is the client store every app has: a [`ConnectionState`], a [`ChatsStore`], a
//!   [`MessagesStore`] and an [`ApprovalsStore`], kept current through a [`Transport`] in
//!   `patchwork-protocol`'s types. The phone reaches its Patchwork with a [`RemoteTransport`].
//! - With the `local` feature, [`local::AppState`] extends it for the desktop, over a `Core` in
//!   the same process: a `SettingsStore`, a `ProjectsStore`, an `AgentsStore`, a `QuiltsStore`,
//!   a `ChatAppsStore` and a `GitModel`, with [`local::core`] as the screens' only way to the
//!   core.

// Rust guideline compliant 2026-10-09

mod session;
mod stores;
pub mod transport;

#[cfg(feature = "local")]
pub mod local;

pub use session::Session;
pub use stores::{ApprovalsStore, ChatsStore, ConnectionState, MessagesStore, Pairing, PendingApproval};
pub use transport::{ClientError, RemoteTransport, Transport};

#[cfg(feature = "local")]
pub use local::{
    AgentsStore, AppState, ChatAppsStore, GitModel, ProjectsStore, QuiltsStore, ReplyFiles,
    SettingsStore,
};

/// Sets up the desktop's store over `core`; call once at startup, before any window opens.
#[cfg(feature = "local")]
pub fn init_local(core: std::sync::Arc<patchwork_core::Core>, cx: &mut gpui::App) {
    local::init(core, cx);
}
