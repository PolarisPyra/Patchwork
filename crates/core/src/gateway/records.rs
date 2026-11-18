//! The pairings and the chats bound to threads, kept in the app state table so they last
//! across launches.

// Rust guideline compliant 2026-10-09

use std::sync::PoisonError;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::platform::Platform;
use super::types::{Binding, ChatAppCall, Pairing};
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::system::now_ms;
use crate::{Error, Core};

/// The app state key of the pairings.
const PAIRED: &str = "gateway.pairings";

/// The app state key of the chats bound to threads.
const BINDINGS: &str = "gateway.bindings";

/// The app state key of the ids of the dispatcher threads.
const DISPATCHERS: &str = "gateway.dispatchers";

/// The app state key of the ids of the threads the chat apps started, which run in the background.
const OPENED: &str = "gateway.opened";

/// The app state key of the threads a conversation is told about without talking to them.
const WATCHES: &str = "gateway.watches";

/// The app state key of what the chat apps had Patchwork do.
const ACTIVITY: &str = "gateway.activity";

/// How many of those are kept: the latest.
const ACTIVITY_KEPT: usize = 200;

impl Core {
    /// What the chat apps had Patchwork do, the latest first.
    #[must_use]
    pub fn chat_app_activity(&self) -> Vec<ChatAppCall> {
        let mut calls: Vec<ChatAppCall> = self.read_list(ACTIVITY);
        calls.reverse();
        calls
    }

    /// Records that the dispatcher `thread_id` ran `capability`, and whether it worked.
    pub(crate) fn note_capability(
        &self,
        thread_id: &str,
        capability: &str,
        permission: &str,
        ok: bool,
    ) {
        let chat = self.bound_chats(thread_id).into_iter().next();
        let who = chat.as_ref().map_or_else(String::new, |chat| {
            self.pairings()
                .into_iter()
                .filter(|u| u.platform == chat.platform && u.chat.as_deref() == Some(&chat.chat))
                .map(|u| u.name)
                .collect::<Vec<_>>()
                .join(", ")
        });
        let call = ChatAppCall {
            at: now_ms(),
            platform: chat.map(|chat| chat.platform),
            who,
            capability: capability.to_string(),
            permission: permission.to_string(),
            ok,
        };
        let saved = self.change_list(ACTIVITY, |calls: &mut Vec<ChatAppCall>| {
            calls.push(call);
            let extra = calls.len().saturating_sub(ACTIVITY_KEPT);
            calls.drain(..extra);
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not record what a chat app ran: {{error}}");
        }
    }

    /// The pairings: who may use Patchwork from each chat app.
    #[must_use]
    pub fn pairings(&self) -> Vec<Pairing> {
        self.read_list(PAIRED)
    }

    /// Stops `id` on `platform` from using Patchwork until they pair again.
    ///
    /// # Errors
    ///
    /// Returns an error when the change cannot be saved.
    pub fn remove_pairing(&self, platform: Platform, id: &str) -> Result<(), Error> {
        self.change_list(PAIRED, |paired: &mut Vec<Pairing>| {
            paired.retain(|u| u.platform != platform || u.id != id);
        })
    }

    pub(super) fn is_paired(&self, platform: Platform, id: &str) -> bool {
        self.pairings()
            .iter()
            .any(|u| u.platform == platform && u.id == id)
    }

    pub(super) fn add_pairing(
        &self,
        platform: Platform,
        id: &str,
        name: &str,
        chat: &str,
    ) -> Result<(), Error> {
        self.change_list(PAIRED, |paired: &mut Vec<Pairing>| {
            paired.retain(|u| u.platform != platform || u.id != id);
            paired.push(Pairing {
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
            .pairings()
            .iter()
            .any(|u| u.platform == platform && u.id == id && u.chat.as_deref() == Some(chat));
        if known {
            return;
        }
        let saved = self.change_list(PAIRED, |paired: &mut Vec<Pairing>| {
            if let Some(entry) = paired
                .iter_mut()
                .find(|u| u.platform == platform && u.id == id)
            {
                entry.chat = Some(chat.to_string());
            }
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not save a pairing's chat: {{error}}");
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
            log::warn!(error:% = error; "could not save a conversation's thread: {{error}}");
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
            log::warn!(error:% = error; "could not save a conversation's thread: {{error}}");
        }
        self.unwatch(thread_id);
        let saved = self.change_list(OPENED, |list: &mut Vec<String>| {
            list.retain(|id| id != thread_id);
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not forget a chat app's thread: {{error}}");
        }
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

    /// Whether `thread_id` was started from a chat app, so it runs in the background: the window
    /// neither lists it nor notifies about it. Dispatchers count, since only chat apps start them.
    pub(crate) fn in_background(&self, thread_id: &str) -> bool {
        self.background_threads().iter().any(|id| id == thread_id)
    }

    /// The ids of every thread started from a chat app. See `in_background`.
    pub(crate) fn background_threads(&self) -> Vec<String> {
        let mut ids = self.read_list::<String>(OPENED);
        ids.extend(self.read_list::<String>(DISPATCHERS));
        ids
    }

    /// Records that a chat app started `thread_id`.
    pub(super) fn mark_opened(&self, thread_id: &str) -> Result<(), Error> {
        self.change_list(OPENED, |list: &mut Vec<String>| {
            list.push(thread_id.to_string());
        })
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
