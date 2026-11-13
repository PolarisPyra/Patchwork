//! Running the tools a chat's agent drives other chats with (see `crate::threads::chat_tools`):
//! each acts on a chat in the caller's project as a person would from the chat list.

// Rust guideline compliant 2026-10-09

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};

use super::wait::Unanswered;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::orchestration::Runner;
use crate::provider::AgentKind;
use crate::threads::chat_tools::{self, LIST, SEND, START, WAIT};
use crate::threads::{NewThread, Role, Thread};
use crate::{Error, Server};

/// The longest `wait_for_chat` waits. Under the 65 minutes a CLI waits on one of Patchwork's own
/// tools, so the caller hears that it ran out rather than a dropped call.
const MAX_WAIT: Duration = Duration::from_secs(60 * 60);

/// A text argument, trimmed, or empty when it is missing.
fn text<'a>(args: &'a Value, key: &str) -> &'a str {
    args[key].as_str().map(str::trim).unwrap_or_default()
}

/// A text argument that must be given.
fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, Error> {
    let value = text(args, key);
    if value.is_empty() {
        return Err(format!("Give the {key}").into());
    }
    Ok(value)
}

/// Whether the chat is replying or about to.
fn busy(runner: &Runner, thread_id: &str) -> bool {
    runner.is_starting(thread_id) || runner.is_running(thread_id)
}

impl Runner {
    /// Runs the chat tool `tool` for the agent of `thread_id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is not offered the tools, an argument is missing or names
    /// a chat outside the caller's project, or the chat cannot be started or messaged.
    pub(crate) fn run_chat_tool(
        &self,
        server: &Arc<Server>,
        thread_id: &str,
        tool: &str,
        args: &Value,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        let caller = server
            .store
            .thread(thread_id)
            .ok_or("This chat is no longer here")?;
        if !chat_tools::offered(server, &caller) {
            return Err("This chat cannot drive other chats".into());
        }
        match tool {
            LIST => Ok(self.list_chats(server, &caller)),
            START => start_chat(server, &caller, args),
            SEND => {
                let target = target(server, &caller, args)?;
                if busy(self, &target.id) {
                    return Err("That chat is still replying; wait for it first".into());
                }
                server.send_message(
                    target.id,
                    required(args, "prompt")?.to_string(),
                    Vec::new(),
                    false,
                )?;
                Ok("Sent. Call wait_for_chat for the reply.".into())
            }
            WAIT => self.wait_for_chat(server, &caller, args, stopped),
            _ => Err(format!("Patchwork has no tool {tool}").into()),
        }
    }

    /// The other chats in the caller's project, newest first.
    fn list_chats(&self, server: &Server, caller: &Thread) -> String {
        let chats: Vec<Value> = server
            .store
            .threads()
            .into_iter()
            .filter(|t| {
                t.id != caller.id
                    && t.parent.is_none()
                    && t.archived_at.is_none()
                    && t.project_id == caller.project_id
            })
            .map(|t| {
                json!({
                    "chat_id": t.id,
                    "title": t.title,
                    "agent": t.agent,
                    "model": t.model,
                    "replying": busy(self, &t.id),
                })
            })
            .collect();
        if chats.is_empty() {
            return "There are no other chats in this project.".into();
        }
        Value::Array(chats).to_string()
    }

    /// Waits for the reply to the last message of the chat `args` names, while the caller's
    /// turn runs. The chat itself is left running when the caller stops waiting.
    fn wait_for_chat(
        &self,
        server: &Arc<Server>,
        caller: &Thread,
        args: &Value,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        let target = target(server, caller, args)?;
        let flags = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            let turn = turns
                .get(&caller.id)
                .ok_or("This turn is no longer running")?;
            [Arc::clone(&turn.cancelled), Arc::clone(&turn.exited)]
        };
        let gone = || stopped() || flags.iter().any(|f| f.load(Ordering::SeqCst));
        let asked = target
            .messages
            .iter()
            .rfind(|m| m.role == Role::User && !m.queued)
            .map(|m| m.id.clone())
            .ok_or("That chat has no messages yet")?;
        let reply = self
            .wait_for_reply(server, &target.id, Some(&asked), &gone, MAX_WAIT)
            .map_err(|why| {
                Error::from(match why {
                    Unanswered::Stopped => "Stopped waiting with this turn",
                    Unanswered::TimedOut => "That chat was still replying after an hour",
                    Unanswered::NotStarted => "That chat has not started replying",
                })
            })?;
        Ok(json!({
            "status": reply.status,
            "reply": reply.text.trim(),
            "error": reply.error,
            "files_changed": reply.files,
            "checks": reply.checks.as_ref().map(|c| c.status),
            "review": reply.review.as_ref().map(|r| json!({"status": r.status, "text": r.text})),
        })
        .to_string())
    }
}

/// The chat `args` names, which must be another chat a person started in the caller's project.
fn target(server: &Server, caller: &Thread, args: &Value) -> Result<Thread, Error> {
    let id = required(args, "chat_id")?;
    if id == caller.id {
        return Err("That is this chat".into());
    }
    server
        .store
        .thread(id)
        .filter(|t| t.parent.is_none() && t.project_id == caller.project_id)
        .ok_or_else(|| "No chat with that id in this project".into())
}

/// Starts a chat in the caller's project with the caller's access, and sends it the prompt.
fn start_chat(server: &Arc<Server>, caller: &Thread, args: &Value) -> Result<String, Error> {
    let prompt = required(args, "prompt")?.to_string();
    let agent = match text(args, "agent") {
        "" => caller.agent,
        name => serde_json::from_value::<AgentKind>(json!(name.to_lowercase()))
            .map_err(|e| format!("Patchwork has no agent {name}: {e}"))?,
    };
    let given = |key: &str| Some(text(args, key).to_string()).filter(|s| !s.is_empty());
    let thread = server.create_thread(NewThread {
        id: None,
        project_id: caller.project_id.clone(),
        agent,
        model: given("model"),
        access_mode: caller.access_mode,
        effort: given("effort"),
        compact_window: None,
        plan_mode: false,
    })?;
    let thread = server.store.thread(&thread.id).unwrap_or(thread);
    let id = thread.id.clone();
    server.emit(AgentEventEnvelope {
        thread_id: id.clone(),
        event: AgentEvent::ThreadCreated { thread },
    });
    // A chat that cannot start leaves no empty chat behind.
    if let Err(e) = server.send_message(id.clone(), prompt, Vec::new(), false) {
        server.delete_thread(&id);
        server.emit(AgentEventEnvelope {
            thread_id: id,
            event: AgentEvent::ThreadDeleted,
        });
        return Err(e);
    }
    Ok(json!({"chat_id": id}).to_string())
}
