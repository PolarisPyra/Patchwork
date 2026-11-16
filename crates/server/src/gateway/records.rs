//! The paired users and the chats bound to threads, kept in the app state table so they last
//! across launches.

// Rust guideline compliant 2026-10-09

use std::sync::PoisonError;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::platform::Platform;
use super::types::{Binding, PairedUser};
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::system::now_ms;
use crate::{Error, Server};

/// The app state key of the paired users.
const USERS: &str = "gateway.users";

/// The app state key of the chats bound to threads.
const BINDINGS: &str = "gateway.bindings";

impl Server {
    /// The people allowed to use Patchwork from a chat app.
    #[must_use]
    pub fn gateway_users(&self) -> Vec<PairedUser> {
        self.read_list(USERS)
    }

    /// Stops `id` on `platform` from using Patchwork until they pair again.
    ///
    /// # Errors
    ///
    /// Returns an error when the change cannot be saved.
    pub fn remove_gateway_user(&self, platform: Platform, id: &str) -> Result<(), Error> {
        self.change_list(USERS, |users: &mut Vec<PairedUser>| {
            users.retain(|u| u.platform != platform || u.id != id);
        })
    }

    pub(super) fn is_paired(&self, platform: Platform, id: &str) -> bool {
        self.gateway_users()
            .iter()
            .any(|u| u.platform == platform && u.id == id)
    }

    pub(super) fn add_gateway_user(
        &self,
        platform: Platform,
        id: &str,
        name: &str,
        chat: &str,
    ) -> Result<(), Error> {
        self.change_list(USERS, |users: &mut Vec<PairedUser>| {
            users.retain(|u| u.platform != platform || u.id != id);
            users.push(PairedUser {
                platform,
                id: id.to_string(),
                name: name.to_string(),
                paired_at: now_ms(),
                chat: Some(chat.to_string()),
            });
        })
    }

    /// Remembers `chat` as the conversation `id` on `platform` last wrote from.
    pub(super) fn note_chat(&self, platform: Platform, id: &str, chat: &str) {
        let known = self
            .gateway_users()
            .iter()
            .any(|u| u.platform == platform && u.id == id && u.chat.as_deref() == Some(chat));
        if known {
            return;
        }
        let saved = self.change_list(USERS, |users: &mut Vec<PairedUser>| {
            if let Some(user) = users
                .iter_mut()
                .find(|u| u.platform == platform && u.id == id)
            {
                user.chat = Some(chat.to_string());
            }
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not save a gateway user's chat: {{error}}");
        }
    }

    /// The thread `chat` on `platform` talks to.
    pub(super) fn bound_thread(&self, platform: Platform, chat: &str) -> Option<String> {
        self.read_list::<Binding>(BINDINGS)
            .into_iter()
            .find(|b| b.platform == platform && b.chat == chat)
            .map(|b| b.thread_id)
    }

    /// Every conversation bound to a thread.
    pub(super) fn bindings(&self) -> Vec<Binding> {
        self.read_list(BINDINGS)
    }

    /// The chats that talk to `thread_id`.
    pub(super) fn bound_chats(&self, thread_id: &str) -> Vec<Binding> {
        self.read_list::<Binding>(BINDINGS)
            .into_iter()
            .filter(|b| b.thread_id == thread_id)
            .collect()
    }

    /// Makes `chat` talk to `thread_id`, or to no thread, and tells the window both threads' chat
    /// apps changed.
    pub(super) fn bind_chat(&self, platform: Platform, chat: &str, thread_id: Option<&str>) {
        let mut changed: Vec<String> = thread_id.iter().map(|id| (*id).to_string()).collect();
        let saved = self.change_list(BINDINGS, |list: &mut Vec<Binding>| {
            list.retain(|b| {
                let kept = b.platform != platform || b.chat != chat;
                if !kept && !changed.contains(&b.thread_id) {
                    changed.push(b.thread_id.clone());
                }
                kept
            });
            if let Some(thread_id) = thread_id {
                list.push(Binding {
                    platform,
                    chat: chat.to_string(),
                    thread_id: thread_id.to_string(),
                });
            }
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not save a gateway chat's thread: {{error}}");
            return;
        }
        for thread_id in changed {
            self.emit(AgentEventEnvelope {
                thread_id,
                event: AgentEvent::ChatAppsChanged,
            });
        }
    }

    /// Forgets the chats bound to a thread that was deleted.
    pub(super) fn unbind_thread(&self, thread_id: &str) {
        let saved = self.change_list(BINDINGS, |list: &mut Vec<Binding>| {
            list.retain(|b| b.thread_id != thread_id);
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not save a gateway chat's thread: {{error}}");
        }
    }

    fn read_list<T: DeserializeOwned>(&self, key: &str) -> Vec<T> {
        self.store
            .state(key)
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    fn change_list<T: Serialize + DeserializeOwned>(
        &self,
        key: &str,
        change: impl FnOnce(&mut Vec<T>),
    ) -> Result<(), Error> {
        let _held = self
            .gateway
            .records
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut list = self.read_list(key);
        change(&mut list);
        let json = serde_json::to_string(&list).map_err(|e| e.to_string())?;
        self.store.set_state(key, &json)
    }
}
