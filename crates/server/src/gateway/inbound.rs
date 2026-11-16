//! What Patchwork does with a message from a chat app: pairing a stranger, answering the
//! agent's request, or sending it on to the chat's thread.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;

use super::hub::Place;
use super::platform::Platform;
use super::types::Incoming;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::threads::{Approval, ApprovalStatus, NewThread, Question};
use crate::{Error, Server};

/// Words that allow what the agent asks.
const ALLOW: &[&str] = &["allow", "yes", "y", "ok", "approve"];

/// Words that turn down what the agent asks.
const DENY: &[&str] = &["deny", "no", "n", "reject", "cancel"];

impl Server {
    /// Handles a message from a chat app, on the thread of its connection.
    pub(super) fn gateway_incoming(self: &Arc<Self>, message: &Incoming) {
        if !self.is_paired(message.platform, &message.user) {
            self.pair(message);
            return;
        }
        let (platform, chat) = (message.platform, message.chat.as_str());
        self.note_chat(platform, &message.user, chat);
        let thread = self
            .bound_thread(platform, chat)
            .filter(|id| self.store.thread(id).is_some());
        let text = message.text.trim();
        let command = text.starts_with('/');
        if !command
            && let Some(id) = &thread
            && let Some(approval) = self.waiting_approval(id)
        {
            self.answer(platform, chat, id, &approval, text);
            return;
        }
        if command && self.gateway_command(message, thread.as_deref()) {
            return;
        }
        self.pass_on(message, thread);
    }

    /// Pairs the sender when the message is the pairing code, and otherwise tells them once how
    /// to pair.
    fn pair(&self, message: &Incoming) {
        let (platform, chat) = (message.platform, message.chat.as_str());
        if self.take_gateway_code(&message.text) {
            match self.add_gateway_user(platform, &message.user, &message.name, chat) {
                Ok(()) => self.say(
                    platform,
                    chat,
                    "Paired. Send a message to start a chat in Patchwork, or /help for what else you can do.",
                ),
                Err(error) => {
                    log::warn!(error:% = error; "could not save a paired gateway user: {{error}}");
                }
            }
            return;
        }
        let first = self
            .gateway
            .lock()
            .told
            .insert((platform, message.user.clone()));
        if first {
            self.say(
                platform,
                chat,
                "To use Patchwork here, make a pairing code in Settings → Gateways on your computer and send it here.",
            );
        }
    }

    /// The request the agent in `thread_id` is waiting on the user to answer.
    fn waiting_approval(&self, thread_id: &str) -> Option<Approval> {
        if !self.runner.is_running(thread_id) {
            return None;
        }
        self.store
            .thread(thread_id)?
            .messages
            .iter()
            .rev()
            .flat_map(|m| m.approvals.iter())
            .find(|a| a.status == ApprovalStatus::Pending && !a.auto)
            .cloned()
    }

    /// Answers the agent's request with the user's reply.
    fn answer(
        self: &Arc<Self>,
        platform: Platform,
        chat: &str,
        thread_id: &str,
        approval: &Approval,
        text: &str,
    ) {
        if approval.secret.is_some() {
            self.say(
                platform,
                chat,
                "Enter the secret in Patchwork on your computer. Chat apps never carry it.",
            );
            return;
        }
        let word = text.to_lowercase();
        let decision = if DENY.contains(&word.as_str()) {
            Some((false, HashMap::new()))
        } else if approval.questions.is_empty() {
            ALLOW
                .contains(&word.as_str())
                .then(|| (true, HashMap::new()))
        } else {
            let answers = answers(&approval.questions, text);
            (!answers.is_empty()).then_some((true, answers))
        };
        let Some((allow, answers)) = decision else {
            self.say(platform, chat, "Reply allow or deny.");
            return;
        };
        self.gateway
            .lock()
            .answered
            .insert(approval.id.clone(), (platform, chat.to_string()));
        if let Err(error) = self.respond_approval(thread_id, &approval.id, allow, answers, None) {
            self.gateway.lock().answered.remove(&approval.id);
            self.say(
                platform,
                chat,
                &format!("Patchwork could not answer: {error}"),
            );
        }
    }

    /// Sends the message to `thread`, or to a new thread when the chat has none.
    fn pass_on(self: &Arc<Self>, message: &Incoming, thread: Option<String>) {
        let (platform, chat) = (message.platform, message.chat.as_str());
        let attachments: Vec<_> = message
            .files
            .iter()
            .filter_map(|bytes| self.attach_image(bytes).ok())
            .collect();
        let text = message.text.trim().to_string();
        if text.is_empty() && attachments.is_empty() {
            return;
        }
        let (id, new) = match thread {
            Some(id) => (id, false),
            None => match self.open_gateway_thread(platform, chat) {
                Ok(id) => (id, true),
                Err(error) => {
                    self.say(
                        platform,
                        chat,
                        &format!("Patchwork could not start a chat: {error}"),
                    );
                    return;
                }
            },
        };
        let queue = self.runner.is_running(&id) && !self.store.settings().steer_while_working;
        match self.send_message_from(id.clone(), text, attachments, queue, Some((platform, chat))) {
            Ok(_) => {
                if let Some(connector) = self.gateway.connector(platform) {
                    connector.typing(chat);
                }
            }
            Err(error) => {
                // A chat that could not take its first message leaves nothing behind.
                if new {
                    self.delete_thread(&id);
                    self.unbind_thread(&id);
                    self.emit(AgentEventEnvelope {
                        thread_id: id,
                        event: AgentEvent::ThreadDeleted,
                    });
                }
                self.say(
                    platform,
                    chat,
                    &format!("Patchwork could not send that: {error}"),
                );
            }
        }
    }

    /// Starts a thread for a chat app conversation, with the gateway's project and access, and
    /// binds the conversation to it.
    fn open_gateway_thread(
        self: &Arc<Self>,
        platform: Platform,
        chat: &str,
    ) -> Result<String, Error> {
        let settings = self.store.settings();
        let chosen = self
            .gateway
            .lock()
            .chats
            .get_mut(&(platform, chat.to_string()))
            .and_then(|c| c.project.take());
        let project_id = match chosen {
            Some(Place::Chat) => None,
            Some(Place::Project(id)) => Some(id),
            None => settings.gateway.project_id.clone(),
        }
        .filter(|id| settings.projects.iter().any(|p| &p.id == id));
        let agent = settings.default_agent();
        let thread = self.create_thread(NewThread {
            id: None,
            project_id,
            agent,
            model: settings.tokens.default_models.get(&agent).cloned(),
            access_mode: settings.gateway.access,
            effort: None,
            compact_window: None,
            plan_mode: false,
        })?;
        let thread = self.store.thread(&thread.id).unwrap_or(thread);
        let id = thread.id.clone();
        self.emit(AgentEventEnvelope {
            thread_id: id.clone(),
            event: AgentEvent::ThreadCreated { thread },
        });
        self.bind_chat(platform, chat, Some(&id));
        Ok(id)
    }
}

/// The answers in `text` to `questions`: one line per question, each the numbers of the options
/// picked or the answer in words.
fn answers(questions: &[Question], text: &str) -> HashMap<String, Vec<String>> {
    let parts: Vec<&str> = if questions.len() == 1 {
        vec![text.trim()]
    } else {
        text.split(['\n', ';'])
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect()
    };
    questions
        .iter()
        .zip(parts)
        .map(|(question, part)| {
            let picked: Option<Vec<String>> = part
                .split([',', ' '])
                .filter(|n| !n.is_empty())
                .map(|n| {
                    n.parse::<usize>()
                        .ok()
                        .and_then(|n| n.checked_sub(1))
                        .and_then(|i| question.options.get(i))
                        .map(|o| o.label.clone())
                })
                .collect();
            let answer = match picked {
                Some(labels) if !labels.is_empty() => labels,
                _ => vec![part.to_string()],
            };
            (question.id.clone(), answer)
        })
        .collect()
}
