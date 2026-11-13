//! Creating, forking, clearing and deleting threads.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use crate::Error;
use crate::provider::AgentKind;
use crate::system::{now_ms, new_id};
use crate::threads::{AccessMode, Message, ResumePoint, Thread};
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
pub(super) fn note(id: &str, text: String) {
    THREAD_LOG.text(LogLevel::Info, id, "chat", text);
}

/// The `app_state` key of where a thread's next Claude turn picks its session up.
fn resume_point_key(thread_id: &str) -> String {
    format!("claude-resume-point:{thread_id}")
}

impl Server {
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
        let mut thread = Thread {
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
        self.give_project_granular(&mut thread);
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
    /// and the same agent and settings, in a session of its own so the original thread goes on
    /// unchanged. Claude branches its session at that reply, so the fork keeps every call and
    /// result and reads them from the cache; other agents, and replies without a cursor, are
    /// given the conversation as a transcript on the fork's first turn.
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
        let branch = match (&source.session_id, &source.messages[end].cursor) {
            (Some(session), Some(at)) if source.agent == AgentKind::Claude => Some((
                session.clone(),
                ResumePoint {
                    at: at.clone(),
                    fork: true,
                },
            )),
            _ => None,
        };
        // Message ids are unique across threads, so the copies get new ones.
        let messages = source.messages[..=end]
            .iter()
            .map(|m| {
                Arc::new(Message {
                    id: new_id(),
                    queued: false,
                    background: Vec::new(),
                    // The original thread's checkpoints stay with it, and its cursors name messages
                    // of its own session.
                    turn: None,
                    cursor: None,
                    ..Message::clone(m)
                })
            })
            .collect();
        let thread = Thread {
            id: new_id(),
            title: format!("{} (fork)", source.title),
            // The first turn branches the original's session, and takes the new one it gets.
            session_id: branch.as_ref().map(|(session, _)| session.clone()),
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
        if let Some((_, point)) = &branch {
            self.set_resume_point(&thread.id, Some(point))?;
        }
        note(
            &thread.id,
            format!("forked from {thread_id} at message {message_id}"),
        );
        Ok(thread)
    }

    /// Where the thread's next Claude turn picks its session up, when an undo or a fork set it.
    pub(crate) fn resume_point(&self, thread_id: &str) -> Option<ResumePoint> {
        let json = self.store.state(&resume_point_key(thread_id))?;
        serde_json::from_str(&json).ok()
    }

    /// Sets where the thread's next Claude turn picks its session up, or forgets it for None.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub(crate) fn set_resume_point(
        &self,
        thread_id: &str,
        point: Option<&ResumePoint>,
    ) -> Result<(), Error> {
        let key = resume_point_key(thread_id);
        match point {
            Some(point) => {
                let json = serde_json::to_string(point).map_err(|e| e.to_string())?;
                self.store.set_state(&key, &json)
            }
            None => self.store.clear_state(&key),
        }
    }

    /// Ends what a thread that is about to be deleted has outside the database: its agent and
    /// jobs, the snapshots its turns left in the project's repository, and its API session.
    pub(crate) fn forget_thread(&self, id: &str) {
        self.runner.cancel(id, None);
        self.runner.forget_jobs(id);
        self.remove_checkpoints(id);
        self.remove_api_session(id);
    }

    /// Permanently deletes a thread and its messages.
    pub fn delete_thread(&self, id: &str) {
        self.forget_thread(id);
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
            self.forget_thread(&child.id);
            if let Err(e) = self.store.delete_thread(&child.id) {
                log::warn!(error:% = e; "failed to delete a subagent's thread: {{error}}");
            }
        });
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
