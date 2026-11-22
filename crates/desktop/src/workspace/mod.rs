//! The window: the toolbar, the activity bar, the sidebar, the chat and the status bar, over the
//! state they all read.
//!
//! The files here are the window itself: its state, its regions laid out, the bars around them,
//! what its shortcuts do, and the update card. `dock/` is the tabbed area under the chat that holds
//! the terminal, the logs and the screen; `hosts/` drives what other crates draw in the window.

// Rust guideline compliant 2026-10-09

mod shortcuts;
mod bars;
pub(crate) mod dock;
mod hosts;
mod layout;
mod window;
mod updates;

pub use patchwork_modals::granular::GranularScope;
pub(crate) use window::{Place, end_thinking, message_mut};
pub(crate) use updates::restart;
pub use window::{
    Page, ReplyNotice, SIDEBAR_MAX, SIDEBAR_MIN, SIDEBAR_WIDTH, View, Workspace, is_busy,
    is_working,
};
