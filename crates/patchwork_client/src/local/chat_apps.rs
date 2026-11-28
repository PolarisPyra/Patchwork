//! The chat apps (Discord, Slack, Mattermost) in the desktop store: which chats they are on, and
//! where each connection stands.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;

use patchwork_core::Core;
use patchwork_core::gateway::{ChatApps, Platform, Status};
use patchwork_gateway::CoreHandoff;

use super::AppState;

/// The chat apps each chat came from or has on it now, and where each connection stands.
#[derive(Debug, Clone, Default)]
pub struct ChatAppsStore {
    /// The chat apps on each chat, by chat id.
    pub by_chat: Arc<HashMap<String, ChatApps>>,
    /// Where each chat app's connection stands; one that is not listed is off.
    pub status: Arc<Vec<(Platform, Status)>>,
}

impl ChatAppsStore {
    pub(super) fn read(core: &Core) -> Self {
        Self {
            by_chat: Arc::new(core.chat_chat_apps()),
            status: Arc::new(core.gateway_status()),
        }
    }

    /// The chat apps on chat `id`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&ChatApps> {
        self.by_chat.get(id)
    }
}

impl AppState {
    /// Reads the chats' chat apps and each connection's status again.
    pub fn reload_chat_apps(&mut self) {
        self.chat_apps = ChatAppsStore::read(&self.core);
    }
}
