//! The chat list and a chat's place in it: order, name, archive, favorite and pin.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use crate::Error;
use crate::system::now_ms;
use crate::chats::{ApprovalStatus, LastPlace, Chat};
use crate::chats::title::clean_name;
use crate::Core;

use super::lifecycle::note;

/// `app_state` key for the chat the user had open last.
const LAST_CHAT_KEY: &str = "last_chat_id";

/// `app_state` key for what filled the window when the app last closed.
const LAST_PLACE_KEY: &str = "last_place";

/// `chat` with only its last message and those still streaming, running background tasks,
/// queued, or waiting on an approval, so listing every chat does not send each one's whole history.
fn chat_summary(chat: &Chat) -> Chat {
    let last = chat.messages.len().saturating_sub(1);
    let messages = chat
        .messages
        .iter()
        .enumerate()
        .filter(|(i, m)| {
            *i == last
                || m.is_streaming()
                || m.background.iter().any(|task| task.status.is_live())
                || m.queued
                || m.approvals
                    .iter()
                    .any(|a| a.status == ApprovalStatus::Pending)
        })
        .map(|(_, m)| Arc::clone(m))
        .collect();
    chat.with_messages(messages)
}

impl Core {
    /// Every chat, each with only the messages the task list reads: the last one, and any
    /// still streaming, running background tasks, queued, or waiting on an approval. `chat` returns one in full.
    ///
    /// Chats a chat app started run in the background and are left out.
    pub fn list_chats(&self) -> Vec<Arc<Chat>> {
        let background = self.background_chats();
        let mut ts = self.store.summaries(chat_summary);
        ts.retain(|t| !background.contains(&t.id));
        ts.sort_by_key(|t| t.position);
        ts
    }

    /// One chat with all its messages.
    ///
    /// # Errors
    ///
    /// Returns an error when no chat has that id.
    pub fn chat(&self, id: &str) -> Result<Chat, Error> {
        Ok(self.store.chat(id).ok_or(super::NOT_FOUND)?)
    }

    /// Puts `ids` in `folder_id` (None: the top level) in their new order.
    ///
    /// # Errors
    ///
    /// Returns an error when the folder is unknown or the new order cannot be saved.
    pub fn reorder_chats(&self, ids: &[String], folder_id: Option<&str>) -> Result<(), Error> {
        if let Some(folder_id) = folder_id {
            self.folder(folder_id)?;
        }
        self.store.reorder_chats(ids, folder_id)
    }

    /// The chat that was open when the app last closed, if one was recorded.
    pub fn last_chat_id(&self) -> Option<String> {
        self.store.state(LAST_CHAT_KEY)
    }

    /// Remembers `id` as the chat open last, to open it at the next launch.
    ///
    /// # Errors
    ///
    /// Returns an error when the value cannot be saved to the database.
    pub fn set_last_chat_id(&self, id: &str) -> Result<(), Error> {
        self.store.set_state(LAST_CHAT_KEY, id)
    }

    /// What filled the window when the app last closed, if it was recorded and can be read.
    pub fn last_place(&self) -> Option<LastPlace> {
        let saved = self.store.state(LAST_PLACE_KEY)?;
        serde_json::from_str(&saved)
            .inspect_err(|e| log::warn!(error:% = e; "could not read the last place: {{error}}"))
            .ok()
    }

    /// Remembers `place` as what fills the window, to open it at the next launch.
    ///
    /// # Errors
    ///
    /// Returns an error when the value cannot be saved to the database.
    pub fn set_last_place(&self, place: &LastPlace) -> Result<(), Error> {
        let saved = serde_json::to_string(place).map_err(|e| e.to_string())?;
        self.store.set_state(LAST_PLACE_KEY, &saved)
    }

    /// Renames the chat `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the title is empty, the chat is unknown, or the change cannot be
    /// saved.
    pub(crate) fn rename_chat(&self, id: &str, title: &str) -> Result<(), Error> {
        let title = clean_name(title)?;
        self.chat(id)?;
        self.store.update_chat(id, |t| t.title = title)?;
        note(id, "renamed".into());
        Ok(())
    }

    /// Moves a chat to the archive, or back to the task list. Archiving stops its running turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown or the change cannot be saved.
    pub(crate) fn set_chat_archived(&self, id: &str, archived: bool) -> Result<(), Error> {
        self.chat(id)?;
        if archived {
            self.runner.cancel(id);
            crate::logging::CHAT_LOG.forget(id);
        }
        self.store
            .update_chat(id, |t| t.archived_at = archived.then(now_ms))?;
        note(id, if archived { "archived" } else { "unarchived" }.into());
        Ok(())
    }

    /// Lists a chat or task under Favorites at the top of the sidebar, or back in its usual spot.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown or the change cannot be saved.
    pub fn set_chat_favorite(&self, id: &str, favorite: bool) -> Result<(), Error> {
        self.chat(id)?;
        self.store.update_chat(id, |t| t.favorite = favorite)
    }

    /// Lists a task first in its project, or back in its folder. Only tasks in a project pin.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is not in a project, is unknown, or the change cannot be
    /// saved.
    pub fn set_chat_pinned(&self, id: &str, pinned: bool) -> Result<(), Error> {
        if pinned && self.chat(id)?.project_id.is_none() {
            return Err("Only tasks in a project can be pinned".into());
        }
        self.store.update_chat(id, |t| t.pinned = pinned)
    }
}
