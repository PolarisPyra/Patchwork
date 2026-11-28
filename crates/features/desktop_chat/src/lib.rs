//! Patchwork's chat screen: the open chat's transcript, the panes and tabs it is split into, the
//! strips and cards above the composer, and the composer a message is written in.
//!
//! `transcript/` is the message list and each reply, `top_right/` what floats over the
//! transcript's top right, `above_composer/` the strips and cards between the transcript and the
//! composer, `worktree_switch.rs` the worktree pieces the composer shows, `tabs.rs` the strip of
//! chats open in tabs, `panes/` the chat area split into panes, and `composer/` the composer.
//!
//! The chat's state lives in a [`ChatUi`] the window holds and derefs to. What only the window
//! can do, such as navigating or opening Settings, the chat asks for through [`ChatWindow`];
//! `use patchwork_desktop_chat::prelude::*` brings the chat's methods on the window into scope.

#![expect(
    missing_debug_implementations,
    reason = "the chat's state holds views, handles and tasks that have no useful debug form"
)]
#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

// Rust guideline compliant 2026-11-27

pub mod above_composer;
pub mod backdrop;
pub mod composer;
pub mod host;
pub mod layout;
pub mod panes;
pub mod prelude;
pub mod session;
pub mod tabs;
pub mod top_right;
pub mod transcript;
pub mod window;
pub mod worktree_marks;
pub mod worktree_switch;

pub use composer::{Composer, Picks};
pub use host::{ChatEvent, ChatHost, Outgoing, WorktreeParts};
pub use layout::{ChatView, OPEN_HOLD};
pub use window::{ChatUi, ChatWindow, MenuTarget, ReplyNotice};
