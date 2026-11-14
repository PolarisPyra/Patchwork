//! Sending a request to the chat it belongs in, without the person choosing one: a request that
//! names a chat goes to that chat, and any other starts a chat in the project it names.
//!
//! A small model reads the request beside the projects and the recent chats and picks. When none
//! answers, a chat or project is taken from its name being in the request.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::projects::Project;
use crate::threads::title::{self, Request, Writer};
use crate::threads::{NewThread, Thread};
use crate::{Error, Server};

/// The most chats the model picks among, newest first: enough for the day's work, and short
/// enough to read in one quick call.
const MAX_CHATS: usize = 40;

/// The shortest title a request is searched for without a model. A shorter one, such as "Fix",
/// is in too many requests to mean its chat.
const MIN_TITLE_CHARS: usize = 6;

const SCHEMA: &str = r#"{"type":"object","properties":{"chat_id":{"type":"string"},"project":{"type":"string"},"prompt":{"type":"string"}},"required":["chat_id","project","prompt"],"additionalProperties":false}"#;

const PROMPT: &str = "You route a request to the coding chat it belongs in. Do not answer the request or carry it out.
Return JSON with the keys chat_id, project and prompt.
- chat_id: the id of a listed chat, when the request names that chat or plainly carries on its work. Otherwise an empty string, which starts a new chat. When unsure, leave it empty.
- project: the listed project the request names, written as listed. An empty string when it names none.
- prompt: the request as the instruction the chat's agent gets, without the words that only say which project or chat it is for. Change nothing else.";

/// Where a dispatched request went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// A chat was started with it.
    Started,
    /// It was sent to a chat already there.
    Sent,
}

/// A request that was dispatched: the chat that has it, and how it got there.
#[derive(Debug, Clone)]
pub struct Dispatched {
    pub thread_id: String,
    pub route: Route,
}

/// What a request was read as: the chat it is for, else the project a new chat starts in, and
/// the message to send.
struct Reading {
    chat_id: Option<String>,
    project_id: Option<String>,
    prompt: String,
}

impl Server {
    /// Sends `text` to the chat it names, or starts a chat like `new` in the project it names.
    ///
    /// The project of `new` is used when the request names none. A chat still replying gets the
    /// message queued behind its reply. Asks a small model, so call it off the UI thread.
    ///
    /// # Errors
    ///
    /// Returns an error when the request is empty, or the chat cannot be made or messaged.
    pub fn dispatch(self: &Arc<Self>, text: &str, mut new: NewThread) -> Result<Dispatched, Error> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Write what to do".into());
        }
        let projects = self.store.settings().projects;
        let chats = self.recent_chats();
        let reading = self
            .read_with_model(text, &new, &projects, &chats)
            .unwrap_or_else(|| read_by_name(text, &projects, &chats));
        if let Some(thread_id) = reading.chat_id {
            self.send_message(thread_id.clone(), reading.prompt, Vec::new(), true)?;
            return Ok(Dispatched {
                thread_id,
                route: Route::Sent,
            });
        }
        if reading.project_id.is_some() {
            new.project_id = reading.project_id;
        }
        let thread = self.create_thread(new)?;
        let thread_id = thread.id.clone();
        self.emit(AgentEventEnvelope {
            thread_id: thread_id.clone(),
            event: AgentEvent::ThreadCreated { thread },
        });
        // A chat that cannot start leaves no empty chat behind.
        if let Err(e) = self.send_message(thread_id.clone(), reading.prompt, Vec::new(), false) {
            self.delete_thread(&thread_id);
            self.emit(AgentEventEnvelope {
                thread_id,
                event: AgentEvent::ThreadDeleted,
            });
            return Err(e);
        }
        Ok(Dispatched {
            thread_id,
            route: Route::Started,
        })
    }

    /// The chats a request may be for: those a person started and has not archived, newest first.
    fn recent_chats(&self) -> Vec<Arc<Thread>> {
        let mut chats: Vec<_> = self
            .list_threads()
            .into_iter()
            .filter(|t| t.parent.is_none() && t.archived_at.is_none())
            .collect();
        chats.sort_by_key(|t| std::cmp::Reverse(last_active(t)));
        chats.truncate(MAX_CHATS);
        chats
    }

    /// What the title writer's model reads the request as. None when no model answers as asked.
    fn read_with_model(
        &self,
        text: &str,
        new: &NewThread,
        projects: &[Project],
        chats: &[Arc<Thread>],
    ) -> Option<Reading> {
        let tokens = self.store.settings().tokens;
        let writer = Writer {
            model: tokens.title_model,
            local_only: tokens.titles_local_only,
        };
        let project_name = |id: Option<&String>| {
            id.and_then(|id| projects.iter().find(|p| &p.id == id))
                .map_or("none", |p| p.name.as_str())
        };
        let listed_projects: Vec<String> = projects
            .iter()
            .map(|project| format!("- {}", project.name))
            .collect();
        let listed_chats: Vec<String> = chats
            .iter()
            .map(|chat| {
                format!(
                    "- id: {} | project: {} | title: {}",
                    chat.id,
                    project_name(chat.project_id.as_ref()),
                    chat.title
                )
            })
            .collect();
        let input = format!(
            "Projects:\n{}\n\nChats, newest first:\n{}\n\nRequest:\n{text}",
            listed_projects.join("\n"),
            listed_chats.join("\n")
        );
        let request = Request {
            prompt: PROMPT,
            schema: SCHEMA,
            label: "What to route",
            input: &input,
        };
        let answer = title::ask(&request, Some(new.agent), new.model.as_deref(), &writer)?;
        // An API model that answered in plain text picked nothing.
        let answer = answer.as_object()?;
        let field = |key: &str| answer.get(key).and_then(|v| v.as_str()).map(str::trim);
        let chat_id = field("chat_id")
            .filter(|id| chats.iter().any(|c| c.id == *id))
            .map(String::from);
        let project_id = field("project")
            .and_then(|name| projects.iter().find(|p| p.name.eq_ignore_ascii_case(name)))
            .map(|p| p.id.clone());
        let prompt = field("prompt").filter(|p| !p.is_empty()).unwrap_or(text);
        Some(Reading {
            chat_id,
            project_id,
            prompt: prompt.to_string(),
        })
    }
}

/// When the chat last had a message, or was made.
fn last_active(thread: &Thread) -> u64 {
    thread
        .messages
        .last()
        .map_or(thread.created_at, |m| m.created_at.max(thread.created_at))
}

/// Reads the request by the names in it: the chat whose title it holds, else the project whose
/// name it holds, the longest in each case. The message is the request as written.
fn read_by_name(text: &str, projects: &[Project], chats: &[Arc<Thread>]) -> Reading {
    let lower = text.to_lowercase();
    let chat_id = chats
        .iter()
        .filter(|c| {
            let title = c.title.trim().to_lowercase();
            title.chars().count() >= MIN_TITLE_CHARS && lower.contains(&title)
        })
        .max_by_key(|c| c.title.len())
        .map(|c| c.id.clone());
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .collect();
    let project_id = projects
        .iter()
        .filter(|p| words.contains(&p.name.to_lowercase().as_str()))
        .max_by_key(|p| p.name.len())
        .map(|p| p.id.clone());
    Reading {
        chat_id,
        project_id,
        prompt: text.to_string(),
    }
}
