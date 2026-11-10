//! Creating, listing, changing and removing threads.

// Rust guideline compliant 2026-10-09

use crate::Error;
use crate::provider::AgentKind;
use crate::system::{now_ms, new_id};
use crate::threads::{MessageStatus, AccessMode, ApprovalStatus, Message, Thread};
use crate::threads::title::clean_name;
use crate::logging::THREAD_LOG;
use crate::settings::LogLevel;
use crate::Server;

/// The options for a new task or chat, as the desktop app sends them.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewThread {
    /// Chosen by the window that asked, so the row can be drawn before this call returns. A
    /// thread created without one gets an id here.
    pub id: Option<String>,
    pub project_id: Option<String>,
    pub agent: AgentKind,
    pub model: Option<String>,
    pub access_mode: AccessMode,
    pub effort: Option<String>,
    /// None follows Settings → Tokens.
    #[serde(default)]
    pub compact_window: Option<u64>,
    pub plan_mode: bool,
}

/// Notes a change to the chat `id` in its log.
fn note(id: &str, text: String) {
    THREAD_LOG.text(LogLevel::Info, id, "chat", text);
}

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
    Thread {
        messages,
        ..thread.clone()
    }
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

    /// Makes a new chat and stores it.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown or the thread cannot be saved.
    pub fn create_thread(&self, new: NewThread) -> Result<Thread, Error> {
        let NewThread {
            id,
            project_id,
            agent,
            model,
            access_mode,
            effort,
            compact_window,
            plan_mode,
        } = new;
        if let Some(id) = &project_id {
            self.project_root(id)?;
        }
        let title = if project_id.is_some() {
            "New task"
        } else {
            "New chat"
        };
        let created_at = now_ms();
        let thread = Thread {
            id: id.unwrap_or_else(new_id),
            project_id,
            title: title.into(),
            agent,
            model,
            access_mode,
            effort,
            compact_window,
            background: None,
            session_id: None,
            created_at,
            messages: vec![],
            goal: None,
            position: -(created_at as i64),
            folder_id: None,
            archived_at: None,
            parent: None,
            plan_mode,
            slash_commands: vec![],
            imported_from: None,
            context_pending: false,
            role: None,
            role_pending: false,
            favorite: false,
            pinned: false,
        };
        self.store.insert_threads(vec![thread.clone()])?;
        note(
            &thread.id,
            format!(
                "created: {:?}, model {}, {:?} access, effort {}, plan mode {}, {}",
                thread.agent,
                thread.model.as_deref().unwrap_or("default"),
                thread.access_mode,
                thread.effort.as_deref().unwrap_or("default"),
                thread.plan_mode,
                if thread.project_id.is_some() {
                    "in a project"
                } else {
                    "a chat"
                },
            ),
        );
        Ok(thread)
    }

    /// Starts a new thread with the conversation of `thread_id` up to and including `message_id`,
    /// and the same agent and settings. Its first turn gives the agent that conversation as a
    /// transcript, in a session of its own, so the original thread goes on unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread or the message is not found, or the fork cannot be saved.
    pub fn fork_thread(&self, thread_id: &str, message_id: &str) -> Result<Thread, Error> {
        let source = self.thread(thread_id)?;
        let end = source
            .messages
            .iter()
            .position(|m| m.id == message_id)
            .ok_or("Message not found")?;
        let created_at = now_ms();
        // Message ids are unique across threads, so the copies get new ones.
        let messages = source.messages[..=end]
            .iter()
            .map(|m| Message {
                id: new_id(),
                queued: false,
                background: Vec::new(),
                // The original thread's checkpoints stay with it.
                turn: None,
                ..m.clone()
            })
            .collect();
        let thread = Thread {
            id: new_id(),
            title: format!("{} (fork)", source.title),
            session_id: None,
            created_at,
            messages,
            goal: None,
            position: -(created_at as i64),
            archived_at: None,
            parent: None,
            imported_from: None,
            context_pending: true,
            role_pending: false,
            favorite: false,
            pinned: false,
            ..source
        };
        self.store.insert_threads(vec![thread.clone()])?;
        note(
            &thread.id,
            format!("forked from {thread_id} at message {message_id}"),
        );
        Ok(thread)
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

    /// Permanently deletes a thread and its messages.
    pub fn delete_thread(&self, id: &str) {
        self.runner.cancel(id, None);
        self.remove_checkpoints(id);
        self.remove_api_session(id);
        if self
            .store
            .thread(id)
            .is_some_and(|t| t.project_id.is_none())
        {
            let _ = std::fs::remove_dir_all(self.chat_dir(id));
        }
        if let Err(e) = self.store.delete_thread(id) {
            log::warn!(error:% = e; "failed to delete thread: {{error}}");
        }
        // Its subagents' threads go with it.
        let children = self
            .store
            .threads()
            .into_iter()
            .filter(|t| t.parent.as_ref().is_some_and(|p| p.thread_id == id));
        children.for_each(|child| {
            if let Err(e) = self.store.delete_thread(&child.id) {
                log::warn!(error:% = e; "failed to delete a subagent's thread: {{error}}");
            }
        });
    }

    /// Sets the thread's model. The agent can only change before the first message,
    /// since the conversation session belongs to one CLI. An imported chat has no session yet,
    /// so it can take any agent until its first turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is unknown, its agent would change after the conversation
    /// has started, or the change cannot be saved.
    pub fn set_thread_model(
        &self,
        id: &str,
        agent: AgentKind,
        model: Option<String>,
    ) -> Result<(), Error> {
        let thread = self.thread(id)?;
        if thread.agent != agent && !thread.messages.is_empty() && !thread.context_pending {
            return Err("The agent can't change after the conversation has started".into());
        }
        let changed = thread.agent != agent || thread.model != model;
        let shown = format!(
            "agent {agent:?}, model {}",
            model.as_deref().unwrap_or("default")
        );
        self.store.update_thread(id, |t| {
            t.agent = agent;
            t.model = model;
        })?;
        // An idle agent was started with the old model.
        if changed {
            note(id, shown);
            self.runner.retire_idle(id);
        }
        Ok(())
    }

    /// Takes effect from the thread's next turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is unknown or the change cannot be saved.
    pub fn set_thread_access_mode(&self, id: &str, mode: AccessMode) -> Result<(), Error> {
        let thread = self.thread(id)?;
        self.store.update_thread(id, |t| t.access_mode = mode)?;
        // An idle agent was started with the old setting.
        if thread.access_mode != mode {
            note(id, format!("{mode:?} access"));
            self.runner.retire_idle(id);
        }
        Ok(())
    }

    /// Takes effect from the thread's next turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is unknown or the change cannot be saved.
    pub fn set_thread_plan_mode(&self, id: &str, on: bool) -> Result<(), Error> {
        let thread = self.thread(id)?;
        self.store.update_thread(id, |t| t.plan_mode = on)?;
        // An idle agent was started with the old setting.
        if thread.plan_mode != on {
            note(id, format!("plan mode {on}"));
            self.runner.retire_idle(id);
        }
        Ok(())
    }

    /// Takes effect from the thread's next turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is unknown or the change cannot be saved.
    pub fn set_thread_effort(&self, id: &str, effort: Option<&str>) -> Result<(), Error> {
        let thread = self.thread(id)?;
        self.store
            .update_thread(id, |t| t.effort = effort.map(String::from))?;
        // An idle agent was started with the old setting.
        if thread.effort.as_deref() != effort {
            note(id, format!("effort {}", effort.unwrap_or("default")));
            self.runner.retire_idle(id);
        }
        Ok(())
    }

    /// Takes effect from the thread's next turn, or the API agent's next step. None follows
    /// Settings → Tokens.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is unknown or the change cannot be saved.
    pub fn set_thread_compact_window(&self, id: &str, window: Option<u64>) -> Result<(), Error> {
        let thread = self.thread(id)?;
        self.store
            .update_thread(id, |t| t.compact_window = window)?;
        // An idle agent was started with the old setting.
        if thread.compact_window != window {
            note(id, format!("compact window {window:?}"));
            self.runner.retire_idle(id);
            let tokens = self.store.settings().tokens;
            self.runner.set_compact_window(
                id,
                window.unwrap_or_else(|| tokens.compact_window(thread.agent)),
            );
        }
        Ok(())
    }

    /// Clears a chat: any running turn is cancelled, then its messages and agent session are
    /// deleted. The chat stays in the sidebar, empty.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread's messages and session cannot be deleted from the database.
    pub fn clear_thread(&self, thread_id: &str) -> Result<(), Error> {
        // The agent process goes too, since it holds the session being deleted.
        self.stop_turn(thread_id, false);
        self.remove_checkpoints(thread_id);
        self.remove_api_session(thread_id);
        self.store.clear_thread(thread_id)?;
        note(
            thread_id,
            "cleared: its messages and agent session were deleted".into(),
        );
        Ok(())
    }

    /// Deletes the saved conversation of an API task, which Patchwork keeps itself, and the tool
    /// output saved beside it.
    fn remove_api_session(&self, thread_id: &str) {
        let Some(thread) = self.store.thread(thread_id) else {
            return;
        };
        if let (AgentKind::Api, Some(session)) = (thread.agent, &thread.session_id) {
            let dir = crate::provider::api::config::sessions_dir();
            let _ = std::fs::remove_file(dir.join(format!("{session}.json")));
            if crate::system::fs::is_plain_file_name(session) {
                let _ = std::fs::remove_dir_all(dir.join(session));
            }
        }
    }
}
