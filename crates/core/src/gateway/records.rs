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
use crate::{Error, Core};

/// The app state key of the paired users.
const USERS: &str = "gateway.users";

/// The app state key of the chats bound to threads.
const BINDINGS: &str = "gateway.bindings";

/// The app state key of the ids of the dispatcher threads.
const DISPATCHERS: &str = "gateway.dispatchers";

/// The app state key of the threads a conversation is told about without talking to them.
const WATCHES: &str = "gateway.watches";

impl Core {
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
    pub(crate) fn bound_chats(&self, thread_id: &str) -> Vec<Binding> {
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

    /// Forgets the chats bound to a thread that was deleted, and what else was kept about it.
    pub(super) fn unbind_thread(&self, thread_id: &str) {
        let saved = self.change_list(BINDINGS, |list: &mut Vec<Binding>| {
            list.retain(|b| b.thread_id != thread_id);
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not save a gateway chat's thread: {{error}}");
        }
        self.unwatch(thread_id);
        if self.is_dispatcher(thread_id) {
            let saved = self.change_list(DISPATCHERS, |list: &mut Vec<String>| {
                list.retain(|id| id != thread_id);
            });
            if let Err(error) = saved {
                log::warn!(error:% = error; "could not forget a dispatcher: {{error}}");
            }
        }
    }

    /// Whether `thread_id` is a dispatcher: a chat app's chat whose agent has Patchwork's tools
    /// and nothing of the computer.
    pub(crate) fn is_dispatcher(&self, thread_id: &str) -> bool {
        self.read_list::<String>(DISPATCHERS)
            .iter()
            .any(|id| id == thread_id)
    }

    /// Makes `thread_id` a dispatcher.
    pub(super) fn mark_dispatcher(&self, thread_id: &str) -> Result<(), Error> {
        self.change_list(DISPATCHERS, |list: &mut Vec<String>| {
            list.push(thread_id.to_string());
        })
    }

    /// Has `chat` told when `thread_id` asks something or finishes its reply.
    pub(crate) fn watch_chat(&self, thread_id: &str, platform: Platform, chat: &str) {
        let saved = self.change_list(WATCHES, |list: &mut Vec<Binding>| {
            let watch = Binding {
                platform,
                chat: chat.to_string(),
                thread_id: thread_id.to_string(),
            };
            if !list.contains(&watch) {
                list.push(watch);
            }
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not save a watched chat: {{error}}");
        }
    }

    /// The conversations told about `thread_id`.
    pub(super) fn watchers(&self, thread_id: &str) -> Vec<Binding> {
        self.read_list::<Binding>(WATCHES)
            .into_iter()
            .filter(|b| b.thread_id == thread_id)
            .collect()
    }

    /// Stops telling anyone about `thread_id`.
    pub(super) fn unwatch(&self, thread_id: &str) {
        if self.watchers(thread_id).is_empty() {
            return;
        }
        let saved = self.change_list(WATCHES, |list: &mut Vec<Binding>| {
            list.retain(|b| b.thread_id != thread_id);
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not forget a watched chat: {{error}}");
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
