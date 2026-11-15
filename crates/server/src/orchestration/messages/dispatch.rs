//! Sending a request where the person sent it: to a chat already there, or to a chat started for
//! it in a project, or one in each of several.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::threads::{Attachment, NewThread};
use crate::{Error, Server};

/// Where a dispatched request went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// A chat was started with it, or one in each of several projects.
    Started,
    /// It was sent to a chat already there.
    Sent,
}

/// A request that was dispatched: the chats that have it, and how it got there.
#[derive(Debug, Clone)]
pub struct Dispatched {
    /// One chat, or one for each project the request was split across.
    pub thread_ids: Vec<String>,
    pub route: Route,
}

/// A place a request can go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The chat with this id.
    Chat(String),
    /// A new chat in this project, or in none.
    New(Option<String>),
    /// A new chat in each of these projects, working side by side.
    Split(Vec<String>),
}

impl Server {
    /// Sends `prompt` with `attachments` to `target`: to its chat, or to a chat like `new` started
    /// for it. A request split across projects gives each chat the same attachments.
    ///
    /// A chat still replying gets the message queued behind its reply. A request split across
    /// projects is sent when at least one of its chats starts.
    ///
    /// # Errors
    ///
    /// Returns an error when the request is empty, or no chat can be made or messaged.
    pub fn dispatch_to(
        self: &Arc<Self>,
        prompt: &str,
        target: &Target,
        new: &NewThread,
        attachments: &[Attachment],
    ) -> Result<Dispatched, Error> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("Write what to do".into());
        }
        let project_ids = match target {
            Target::Chat(thread_id) => {
                self.send_message(
                    thread_id.clone(),
                    prompt.to_string(),
                    attachments.to_vec(),
                    true,
                )?;
                return Ok(Dispatched {
                    thread_ids: vec![thread_id.clone()],
                    route: Route::Sent,
                });
            }
            Target::New(project_id) => vec![project_id.clone()],
            Target::Split(project_ids) => project_ids.iter().cloned().map(Some).collect(),
        };
        let mut thread_ids = Vec::new();
        let mut failed = None;
        for project_id in project_ids {
            let new = NewThread {
                id: None,
                project_id,
                ..new.clone()
            };
            match self.start_chat(new, prompt, attachments) {
                Ok(thread_id) => thread_ids.push(thread_id),
                Err(e) => failed = Some(e),
            }
        }
        match failed {
            Some(e) if thread_ids.is_empty() => Err(e),
            _ => Ok(Dispatched {
                thread_ids,
                route: Route::Started,
            }),
        }
    }

    /// Starts a chat with `prompt` and `attachments` as its first message, and gives its id.
    fn start_chat(
        self: &Arc<Self>,
        new: NewThread,
        prompt: &str,
        attachments: &[Attachment],
    ) -> Result<String, Error> {
        let thread = self.create_thread(new)?;
        let thread_id = thread.id.clone();
        self.emit(AgentEventEnvelope {
            thread_id: thread_id.clone(),
            event: AgentEvent::ThreadCreated { thread },
        });
        // A chat that cannot start leaves no empty chat behind.
        if let Err(e) = self.send_message(
            thread_id.clone(),
            prompt.to_string(),
            attachments.to_vec(),
            false,
        ) {
            self.delete_thread(&thread_id);
            self.emit(AgentEventEnvelope {
                thread_id,
                event: AgentEvent::ThreadDeleted,
            });
            return Err(e);
        }
        Ok(thread_id)
    }
}
