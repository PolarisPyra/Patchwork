//! The pairings and the chats bound to chats, kept in the app state table so they last
//! across launches.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::platform::Platform;
use super::types::{Binding, ChatAppCall, Pairing};
use crate::events::{ChatEvent, ChatEventEnvelope};
use crate::system::now_ms;
use crate::{Error, Core};

/// The app state key of the pairings.
const PAIRED: &str = "gateway.pairings";

/// The app state key of the chats bound to chats.
const BINDINGS: &str = "gateway.bindings";

/// The app state key of the ids of the dispatcher chats.
const DISPATCHERS: &str = "gateway.dispatchers";

/// The app state key of the ids of the chats the chat apps started, which run in the background.
const OPENED: &str = "gateway.opened";

/// The app state key of the chats a conversation is told about without talking to them.
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

    /// Records that the dispatcher `chat_id` ran `capability`, and whether it worked.
    pub(crate) fn note_capability(
        &self,
        chat_id: &str,
        capability: &str,
        permission: &str,
        ok: bool,
    ) {
        let conversation = self.bound_conversations(chat_id).into_iter().next();
        let who = conversation
            .as_ref()
            .map_or_else(String::new, |conversation| {
                self.pairings()
                    .into_iter()
                    .filter(|u| {
                        u.platform == conversation.platform
                            && u.conversation.as_deref() == Some(&conversation.conversation)
                    })
                    .map(|u| u.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            });
        let call = ChatAppCall {
            at: now_ms(),
            platform: conversation.map(|conversation| conversation.platform),
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

    pub fn is_paired(&self, platform: Platform, id: &str) -> bool {
        self.pairings()
            .iter()
            .any(|u| u.platform == platform && u.id == id)
    }

    /// Pairs user `id` on `platform`, named `name`, who wrote from `conversation`, in place of
    /// any pairing they had.
    ///
    /// # Errors
    ///
    /// Returns an error when the pairings cannot be saved.
    pub fn add_pairing(
        &self,
        platform: Platform,
        id: &str,
        name: &str,
        conversation: &str,
    ) -> Result<(), Error> {
        self.change_list(PAIRED, |paired: &mut Vec<Pairing>| {
            paired.retain(|u| u.platform != platform || u.id != id);
            paired.push(Pairing {
                platform,
                id: id.to_string(),
                name: name.to_string(),
                paired_at: now_ms(),
                conversation: Some(conversation.to_string()),
            });
        })
    }

    /// Remembers `chat` as the conversation `id` on `platform` last wrote from.
    pub fn note_conversation(&self, platform: Platform, id: &str, conversation: &str) {
        let known = self.pairings().iter().any(|u| {
            u.platform == platform && u.id == id && u.conversation.as_deref() == Some(conversation)
        });
        if known {
            return;
        }
        let saved = self.change_list(PAIRED, |paired: &mut Vec<Pairing>| {
            if let Some(entry) = paired
                .iter_mut()
                .find(|u| u.platform == platform && u.id == id)
            {
                entry.conversation = Some(conversation.to_string());
            }
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not save a pairing's chat: {{error}}");
        }
    }

    /// The chat `chat` on `platform` talks to.
    pub fn bound_chat(&self, platform: Platform, conversation: &str) -> Option<String> {
        self.read_list::<Binding>(BINDINGS)
            .into_iter()
            .find(|b| b.platform == platform && b.conversation == conversation)
            .map(|b| b.chat_id)
    }

    /// Every conversation bound to a chat.
    pub fn bindings(&self) -> Vec<Binding> {
        self.read_list(BINDINGS)
    }

    /// The chats that talk to `chat_id`.
    pub fn bound_conversations(&self, chat_id: &str) -> Vec<Binding> {
        self.read_list::<Binding>(BINDINGS)
            .into_iter()
            .filter(|b| b.chat_id == chat_id)
            .collect()
    }

    /// Makes `chat` talk to `chat_id`, or to no chat, and tells the window both chats' chat
    /// apps changed.
    pub fn bind_conversation(&self, platform: Platform, conversation: &str, chat_id: Option<&str>) {
        let mut changed: Vec<String> = chat_id.iter().map(|id| (*id).to_string()).collect();
        let saved = self.change_list(BINDINGS, |list: &mut Vec<Binding>| {
            list.retain(|b| {
                let kept = b.platform != platform || b.conversation != conversation;
                if !kept && !changed.contains(&b.chat_id) {
                    changed.push(b.chat_id.clone());
                }
                kept
            });
            if let Some(chat_id) = chat_id {
                list.push(Binding {
                    platform,
                    conversation: conversation.to_string(),
                    chat_id: chat_id.to_string(),
                });
            }
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not save a conversation's chat: {{error}}");
            return;
        }
        for chat_id in changed {
            self.emit(ChatEventEnvelope {
                chat_id,
                event: ChatEvent::ChatAppsChanged,
            });
        }
    }

    /// Forgets the chats bound to a chat that was deleted, and what else was kept about it.
    pub fn unbind_chat(&self, chat_id: &str) {
        let saved = self.change_list(BINDINGS, |list: &mut Vec<Binding>| {
            list.retain(|b| b.chat_id != chat_id);
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not save a conversation's chat: {{error}}");
        }
        self.unwatch(chat_id);
        let saved = self.change_list(OPENED, |list: &mut Vec<String>| {
            list.retain(|id| id != chat_id);
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not forget a chat app's chat: {{error}}");
        }
        if self.is_dispatcher(chat_id) {
            let saved = self.change_list(DISPATCHERS, |list: &mut Vec<String>| {
                list.retain(|id| id != chat_id);
            });
            if let Err(error) = saved {
                log::warn!(error:% = error; "could not forget a dispatcher: {{error}}");
            }
        }
    }

    /// Whether `chat_id` is a dispatcher: a chat app's chat whose agent has Patchwork's tools
    /// and nothing of the computer.
    pub fn is_dispatcher(&self, chat_id: &str) -> bool {
        self.read_list::<String>(DISPATCHERS)
            .iter()
            .any(|id| id == chat_id)
    }

    /// Whether `chat_id` was started from a chat app before chat apps had agents, so it runs in
    /// the background: the window neither lists it nor notifies about it. A chat app agent's
    /// runs are listed under the agent.
    pub(crate) fn in_background(&self, chat_id: &str) -> bool {
        self.background_chats().iter().any(|id| id == chat_id)
    }

    /// The ids of every chat a chat app started that is not an agent's run. See `in_background`.
    pub(crate) fn background_chats(&self) -> Vec<String> {
        let mut ids = self.read_list::<String>(OPENED);
        ids.extend(self.read_list::<String>(DISPATCHERS));
        let runs: Vec<String> = self.store.with_chats(|chats| {
            chats
                .iter()
                .filter(|t| t.agent_id.is_some() && ids.contains(&t.id))
                .map(|t| t.id.clone())
                .collect()
        });
        ids.retain(|id| !runs.contains(id));
        ids
    }

    /// Makes `chat_id` a dispatcher.
    pub(super) fn mark_dispatcher(&self, chat_id: &str) -> Result<(), Error> {
        self.change_list(DISPATCHERS, |list: &mut Vec<String>| {
            list.push(chat_id.to_string());
        })
    }

    /// Has `chat` told when `chat_id` asks something or finishes its reply.
    pub(crate) fn watch_conversation(&self, chat_id: &str, platform: Platform, conversation: &str) {
        let saved = self.change_list(WATCHES, |list: &mut Vec<Binding>| {
            let watch = Binding {
                platform,
                conversation: conversation.to_string(),
                chat_id: chat_id.to_string(),
            };
            if !list.contains(&watch) {
                list.push(watch);
            }
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not save a watched chat: {{error}}");
        }
    }

    /// The conversations told about `chat_id`.
    pub fn watchers(&self, chat_id: &str) -> Vec<Binding> {
        self.read_list::<Binding>(WATCHES)
            .into_iter()
            .filter(|b| b.chat_id == chat_id)
            .collect()
    }

    /// Stops telling anyone about `chat_id`.
    pub fn unwatch(&self, chat_id: &str) {
        if self.watchers(chat_id).is_empty() {
            return;
        }
        let saved = self.change_list(WATCHES, |list: &mut Vec<Binding>| {
            list.retain(|b| b.chat_id != chat_id);
        });
        if let Err(error) = saved {
            log::warn!(error:% = error; "could not forget a watched chat: {{error}}");
        }
    }

    pub fn read_list<T: DeserializeOwned>(&self, key: &str) -> Vec<T> {
        self.store
            .state(key)
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// Changes the list kept under `key` and saves it.
    ///
    /// # Errors
    ///
    /// Returns an error when the list cannot be written or saved.
    pub fn change_list<T: Serialize + DeserializeOwned>(
        &self,
        key: &str,
        change: impl FnOnce(&mut Vec<T>),
    ) -> Result<(), Error> {
        let _held = self.chat_app_records.locked();
        let mut list = self.read_list(key);
        change(&mut list);
        let json = serde_json::to_string(&list).map_err(|e| e.to_string())?;
        self.store.set_state(key, &json)
    }
}
