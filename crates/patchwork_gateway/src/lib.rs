//! The gateway: chat apps that reach Patchwork's chats. A pairing's messages go to the chat their
//! conversation is bound to, and the agent's requests and replies come back.
//!
//! The core keeps the gateway's settings, pairings and bindings; this crate runs the connections
//! and answers the core through the [`ChatAppHooks`] that [`install`] hands it.

// Rust guideline compliant 2026-11-27

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
mod outbound;
mod platform;
mod qq;
mod signal;
mod slack;
mod telegram;
mod types;
mod watch;

use std::sync::Arc;

use patchwork_core::chats::Message;
use patchwork_core::events::ChatEventEnvelope;
use patchwork_core::gateway::{ChatAppHooks, Platform, Status};
use patchwork_core::{Core, Error};

pub use control::CoreControl;
pub use handoff::CoreHandoff;
pub use types::{Button, Incoming};

/// The core's gateway methods, for the files of this crate to call.
mod prelude {
    pub(crate) use crate::commands::Commands;
    pub(crate) use crate::control::Control;
    pub(crate) use crate::files::Files;
    pub(crate) use crate::handoff::{CoreHandoff, Handoff};
    pub(crate) use crate::hub::Connections;
    pub(crate) use crate::inbound::Inbound;
    pub(crate) use crate::notify::Notify;
    pub(crate) use crate::outbound::Outbound;
    pub(crate) use crate::watch::Watch;
}

/// The gateway as the core calls it.
struct Hooks;

impl ChatAppHooks for Hooks {
    fn observe(&self, _core: &Core, event: &ChatEventEnvelope) {
        hub::HUB.observe(event);
    }

    fn mirror(&self, chat_id: String, message: &Message, from: Option<(Platform, &str)>) {
        hub::HUB.mirror(chat_id, message, from);
    }

    fn restart(&self, core: &Arc<Core>) {
        control::Control::restart_connections(core);
    }

    fn status(&self) -> Vec<(Platform, Status)> {
        control::status()
    }

    fn new_pairing_code(&self) -> (String, u64) {
        control::make_pairing_code()
    }

    fn send_diff(
        &self,
        core: &Core,
        platform: Platform,
        conversation: &str,
        chat_id: Option<&str>,
    ) -> Option<String> {
        files::Files::send_diff(core, platform, conversation, chat_id)
    }

    fn send_file_to_chats(
        &self,
        core: &Core,
        chat_id: &str,
        name: &str,
        bytes: &[u8],
        caption: &str,
    ) -> Result<usize, Error> {
        files::Files::send_file_to_chats(core, chat_id, name, bytes, caption)
    }

    fn send_picture_to_chats(
        &self,
        core: &Core,
        chat_id: &str,
        stem: &str,
        bytes: Vec<u8>,
        caption: &str,
    ) -> Result<(String, usize), Error> {
        files::Files::send_picture_to_chats(core, chat_id, stem, bytes, caption)
    }

    fn chat_app_visits(&self, core: &Core, caller: &str, target: &str) {
        handoff::Handoff::chat_app_visits(core, caller, target);
    }

    fn chat_app_context(&self, core: &Core, chat_id: &str, text: String) -> String {
        handoff::Handoff::chat_app_context(core, chat_id, text)
    }

    fn drop_reset_notices(&self, core: &Core, chat_id: &str) {
        notify::Notify::drop_reset_notices(core, chat_id);
    }

    fn answer_for_dispatcher(
        &self,
        core: &Arc<Core>,
        dispatcher: &str,
        chat_id: &str,
        text: &str,
    ) -> Result<String, Error> {
        watch::Watch::answer_for_dispatcher(core, dispatcher, chat_id, text)
    }
}

/// Hands `core` the gateway, and opens the chat apps' connections when this is the Patchwork
/// that owns the data folder, so two never both reply.
pub fn install(core: &Arc<Core>) {
    core.set_chat_app_hooks(Arc::new(Hooks));
    if core.store().owns_data() {
        let starting = Arc::clone(core);
        std::thread::spawn(move || starting.restart_gateway());
    }
}
