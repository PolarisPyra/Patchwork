//! Sending a request to the chat it belongs in, without the person choosing one: a request that
//! names a chat goes to that chat, and any other starts a chat in the project it names, or one
//! in each project when it names several.
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

const SCHEMA: &str = r#"{"type":"object","properties":{"chat_id":{"type":"string"},"other_chat_id":{"type":"string"},"projects":{"type":"array","items":{"type":"string"}},"prompt":{"type":"string"}},"required":["chat_id","other_chat_id","projects","prompt"],"additionalProperties":false}"#;

const PROMPT: &str = "You route a request to the coding chat it belongs in. Do not answer the request or carry it out.
Return JSON with the keys chat_id, other_chat_id, projects and prompt.
- chat_id: the id of a listed chat, when the request names that chat or plainly carries on its work. Otherwise an empty string, which starts a new chat. When unsure, leave it empty.
- other_chat_id: the id of a second listed chat the request could as well be for, when you cannot tell which of two is meant. Otherwise an empty string.
- projects: the listed projects the request names, written as listed. Several only when it asks for the same work in each of them. Empty when it names none.
- prompt: the request as the instruction the chat's agent gets, without the words that only say which project or chat it is for. Change nothing else.";

/// The most other places a request is offered beside the one picked.
const MAX_OTHERS: usize = 4;

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

/// Where a request would go, before it is sent.
#[derive(Debug, Clone)]
pub struct Routing {
    /// The message its chat gets.
    pub prompt: String,
    /// The place picked, then the other likely ones. Never empty.
    pub targets: Vec<Target>,
    /// The first two places are as likely as each other, so the person picks.
    pub unsure: bool,
}

/// What a request was read as: the chat it is for, a second it may be for, the projects it
/// names, and the message to send.
struct Reading {
    chat_id: Option<String>,
    other_chat: Option<String>,
    project_ids: Vec<String>,
    prompt: String,
}

impl Server {
    /// Where `text` would go if sent now, without sending it: the chat it names, or a new chat
    /// like `new` in the project it names, and the other places it could go.
    ///
    /// The project of `new` is used when the request names none. Asks a small model, so call it
    /// off the UI thread.
    ///
    /// # Errors
    ///
    /// Returns an error when the request is empty.
    pub fn route(&self, text: &str, new: &NewThread) -> Result<Routing, Error> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Write what to do".into());
        }
        let projects = self.store.settings().projects;
        let chats = self.recent_chats();
        let reading = self
            .read_with_model(text, new, &projects, &chats)
            .unwrap_or_else(|| read_by_name(text, &projects, &chats));
        let named = reading.project_ids.first().cloned();
        let fresh = Target::New(named.clone().or_else(|| new.project_id.clone()));
        let mut targets = Vec::new();
        match (reading.chat_id, reading.project_ids.len()) {
            (Some(chat_id), _) => targets.push(Target::Chat(chat_id)),
            (None, 2..) => targets.push(Target::Split(reading.project_ids.clone())),
            (None, _) => targets.push(fresh.clone()),
        }
        let unsure = reading.other_chat.is_some() && matches!(targets[0], Target::Chat(_));
        targets.extend(reading.other_chat.filter(|_| unsure).map(Target::Chat));
        // The other places: a new chat, then the newest chats, those of the named project first.
        let in_project = |chat: &&Arc<Thread>| named.is_some() && chat.project_id == named;
        let others = std::iter::once(fresh)
            .chain(
                chats
                    .iter()
                    .filter(in_project)
                    .chain(chats.iter().filter(|c| !in_project(c)))
                    .map(|chat| Target::Chat(chat.id.clone())),
            )
            .take(MAX_OTHERS + targets.len());
        for other in others {
            if !targets.contains(&other) {
                targets.push(other);
            }
        }
        Ok(Routing {
            prompt: reading.prompt,
            targets,
            unsure,
        })
    }

    /// Sends `prompt` to `target`: to its chat, or to a chat like `new` started for it.
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
    ) -> Result<Dispatched, Error> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("Write what to do".into());
        }
        let project_ids = match target {
            Target::Chat(thread_id) => {
                self.send_message(thread_id.clone(), prompt.to_string(), Vec::new(), true)?;
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
            match self.start_chat(new, prompt) {
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

    /// Starts a chat with `prompt` as its first message, and gives its id.
    fn start_chat(self: &Arc<Self>, new: NewThread, prompt: &str) -> Result<String, Error> {
        let thread = self.create_thread(new)?;
        let thread_id = thread.id.clone();
        self.emit(AgentEventEnvelope {
            thread_id: thread_id.clone(),
            event: AgentEvent::ThreadCreated { thread },
        });
        // A chat that cannot start leaves no empty chat behind.
        if let Err(e) = self.send_message(thread_id.clone(), prompt.to_string(), Vec::new(), false)
        {
            self.delete_thread(&thread_id);
            self.emit(AgentEventEnvelope {
                thread_id,
                event: AgentEvent::ThreadDeleted,
            });
            return Err(e);
        }
        Ok(thread_id)
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
        let listed = |key: &str| {
            field(key)
                .filter(|id| chats.iter().any(|c| c.id == *id))
                .map(String::from)
        };
        let chat_id = listed("chat_id");
        let other_chat = listed("other_chat_id").filter(|id| Some(id) != chat_id.as_ref());
        let mut project_ids: Vec<String> = Vec::new();
        for name in answer
            .get("projects")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
        {
            let found = projects
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(name.trim()));
            if let Some(project) = found.filter(|p| !project_ids.contains(&p.id)) {
                project_ids.push(project.id.clone());
            }
        }
        let prompt = field("prompt").filter(|p| !p.is_empty()).unwrap_or(text);
        Some(Reading {
            chat_id,
            other_chat,
            project_ids,
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
    let project_ids = projects
        .iter()
        .filter(|p| words.contains(&p.name.to_lowercase().as_str()))
        .max_by_key(|p| p.name.len())
        .map(|p| p.id.clone())
        .into_iter()
        .collect();
    Reading {
        chat_id,
        other_chat: None,
        project_ids,
        prompt: text.to_string(),
    }
}
