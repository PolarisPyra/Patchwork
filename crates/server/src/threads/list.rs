//! The thread list and a thread's place in it: order, name, archive, favorite and pin.

// Rust guideline compliant 2026-10-09

use crate::Error;
use crate::system::now_ms;
use crate::threads::{MessageStatus, ApprovalStatus, Thread};
use crate::threads::title::clean_name;
use crate::Server;

use super::lifecycle::note;

/// `app_state` key for the thread the user had open last.
const LAST_THREAD_KEY: &str = "last_thread_id";

/// `thread` with only its last message and those still streaming, running background tasks,
/// queued, or waiting on an approval, so listing every thread does not send each one's whole history.
fn thread_summary(thread: &Thread) -> Thread {
    let last = thread.messages.len().saturating_sub(1);
    let messages = thread
        .messages
        .iter()
        .enumerate()
        .filter(|(i, m)| {
            *i == last
                || m.status == MessageStatus::Streaming
                || m.background.iter().any(|task| task.status.is_live())
                || m.queued
                || m.approvals
                    .iter()
                    .any(|a| a.status == ApprovalStatus::Pending)
        })
        .map(|(_, m)| m.clone())
        .collect();
    thread.with_messages(messages)
}

impl Server {
    /// Every thread, each with only the messages the task list reads: the last one, and any
    /// still streaming, running background tasks, queued, or waiting on an approval. `thread` returns one in full.
    pub fn list_threads(&self) -> Vec<Thread> {
        let mut ts: Vec<Thread> = self
            .store
            .with_threads(|threads| threads.iter().map(thread_summary).collect());
        ts.sort_by_key(|t| t.position);
        ts
    }

    /// One thread with all its messages.
    ///
    /// # Errors
    ///
    /// Returns an error when no thread has that id.
    pub fn thread(&self, id: &str) -> Result<Thread, Error> {
        Ok(self.store.thread(id).ok_or(super::NOT_FOUND)?)
    }

    /// Puts `ids` in `folder_id` (None: the top level) in their new order.
    ///
    /// # Errors
    ///
    /// Returns an error when the folder is unknown or the new order cannot be saved.
    pub fn reorder_threads(&self, ids: &[String], folder_id: Option<&str>) -> Result<(), Error> {
        if let Some(folder_id) = folder_id {
            self.folder(folder_id)?;
        }
        self.store.reorder_threads(ids, folder_id)
    }

    /// The thread that was open when the app last closed, if one was recorded.
    pub fn last_thread_id(&self) -> Option<String> {
        self.store.state(LAST_THREAD_KEY)
    }

    /// Remembers `id` as the chat open last, to open it at the next launch.
    ///
    /// # Errors
    ///
    /// Returns an error when the value cannot be saved to the database.
    pub fn set_last_thread_id(&self, id: &str) -> Result<(), Error> {
        self.store.set_state(LAST_THREAD_KEY, id)
    }

    /// Renames the chat `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the title is empty, the thread is unknown, or the change cannot be
    /// saved.
    pub fn rename_thread(&self, id: &str, title: &str) -> Result<(), Error> {
        let title = clean_name(title)?;
        self.thread(id)?;
        self.store.update_thread(id, |t| t.title = title)?;
        note(id, "renamed".into());
        Ok(())
    }

    /// Moves a thread to the archive, or back to the task list. Archiving stops its running turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is unknown or the change cannot be saved.
    pub fn set_thread_archived(&self, id: &str, archived: bool) -> Result<(), Error> {
        self.thread(id)?;
        if archived {
            self.runner.cancel(id, None);
        }
        self.store
            .update_thread(id, |t| t.archived_at = archived.then(now_ms))?;
        note(id, if archived { "archived" } else { "unarchived" }.into());
        Ok(())
    }

    /// Lists a chat or task under Favorites at the top of the sidebar, or back in its usual spot.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is unknown or the change cannot be saved.
    pub fn set_thread_favorite(&self, id: &str, favorite: bool) -> Result<(), Error> {
        self.thread(id)?;
        self.store.update_thread(id, |t| t.favorite = favorite)
    }

    /// Lists a task first in its project, or back in its folder. Only tasks in a project pin.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is not in a project, is unknown, or the change cannot be
    /// saved.
    pub fn set_thread_pinned(&self, id: &str, pinned: bool) -> Result<(), Error> {
        if pinned && self.thread(id)?.project_id.is_none() {
            return Err("Only tasks in a project can be pinned".into());
        }
        self.store.update_thread(id, |t| t.pinned = pinned)
    }
}
