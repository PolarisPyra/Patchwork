//! What goes back out to chat apps: a thread's reply as it is written, the agent's requests, and
//! the whole reply when the turn ends.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Weak;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::hub::Out;
use super::files::MESSAGES;
use super::net::chunks;
use super::platform::Platform;
use super::types::{Binding, Button};
use crate::Core;
use crate::provider::clip;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::threads::{Approval, ApprovalStatus, Message, MessageStatus, Role};

/// How often the reply a chat app shows being written changes at most.
const REPLY_EVERY: Duration = Duration::from_millis(1500);

/// How often a working thread shows typing at most.
const TYPING_EVERY: Duration = Duration::from_secs(5);

/// The most characters of a tool's summary or a request's detail a chat app is shown.
const DETAIL_LIMIT: usize = 1500;

/// The most files a reply lists as changed.
const FILES_LISTED: usize = 10;

/// What a chat app was shown of a turn that is working.
#[derive(Default)]
struct Progress {
    /// The reply written so far.
    text: String,
    shown: Option<Instant>,
    typed: Option<Instant>,
    /// Whether the turn was asked for on the computer, so its conversations hear none of it.
    local: bool,
    /// The message holding the reply in each chat, by platform and chat; empty where it cannot
    /// be edited.
    notes: HashMap<(Platform, String), String>,
}

/// Sends the events of bound threads, and the messages the person sends them, to their chats
/// until the server closes.
pub(super) fn forward(core: &Weak<Core>, events: &Receiver<Out>) {
    let mut working: HashMap<String, Progress> = HashMap::new();
    while let Ok(out) = events.recv() {
        let Some(core) = core.upgrade() else {
            return;
        };
        match out {
            Out::Event(event) => core.send_out(event, &mut working),
            Out::Sent {
                thread_id,
                message,
                from,
            } => {
                // A message from a chat app joins a turn asked for on the computer, so the
                // reply goes there too.
                if let (Some(_), Some(progress)) = (&from, working.get_mut(&thread_id)) {
                    progress.local = false;
                }
                core.show_sent(&thread_id, &message, from.as_ref());
            }
        }
    }
}

impl Core {
    /// Sends `text` to `chat`, split to the platform's length.
    pub(super) fn say(&self, platform: Platform, chat: &str, text: &str) {
        self.say_with(platform, chat, text, &[]);
    }

    /// Sends `text` to `chat` with `buttons` under its last piece, and returns the id of that
    /// piece when it can be edited.
    pub(super) fn say_with(
        &self,
        platform: Platform,
        chat: &str,
        text: &str,
        buttons: &[Button],
    ) -> Option<String> {
        let connector = self.gateway.connector(platform)?;
        let pieces = chunks(text, platform.limit());
        let last = pieces.len().saturating_sub(1);
        let mut id = None;
        for (i, piece) in pieces.iter().enumerate() {
            let buttons = if i == last { buttons } else { &[] };
            match connector.send(chat, piece, buttons) {
                Ok(sent) => id = sent,
                Err(error) => {
                    log::warn!(platform = platform.id(), error:% = error; "could not send to {{platform}}: {{error}}");
                    return None;
                }
            }
        }
        id
    }

    fn send_out(&self, envelope: AgentEventEnvelope, working: &mut HashMap<String, Progress>) {
        let thread_id = envelope.thread_id;
        if matches!(envelope.event, AgentEvent::ThreadDeleted) {
            working.remove(&thread_id);
            self.unbind_thread(&thread_id);
            return;
        }
        // A reply is written in many small pieces, so most are only kept.
        let mut writing = None;
        if let AgentEvent::TextDelta { text, .. } = &envelope.event {
            let Some(progress) = working.get_mut(&thread_id).filter(|p| !p.local) else {
                return;
            };
            progress.text.push_str(text);
            if progress.shown.is_some_and(|at| at.elapsed() < REPLY_EVERY)
                || progress.text.trim().is_empty()
            {
                return;
            }
            progress.shown = Some(Instant::now());
            writing = Some(progress.text.clone());
        }
        let mut chats = self.bound_chats(&thread_id);
        if let AgentEvent::MessageStarted { message } = &envelope.event
            && message.role == Role::Assistant
            && !chats.is_empty()
        {
            let local = !self.asked_on_chat_app(&thread_id, &message.id);
            if !local {
                self.chat_app_used(&thread_id);
            }
            let progress = Progress {
                local,
                ..Progress::default()
            };
            working.insert(thread_id.clone(), progress);
        }
        // A turn asked for on the computer is answered there. Its conversations hear of it only
        // as any other chat's would, by the notification settings.
        if working.get(&thread_id).is_some_and(|p| p.local) {
            if matches!(envelope.event, AgentEvent::TurnCompleted { .. }) {
                working.remove(&thread_id);
                self.chat_app_returns(&thread_id);
            }
            chats.clear();
        }
        if chats.is_empty() {
            // A chat a dispatcher's conversation watches is told there; the rest go by the
            // notification settings.
            if !self.tell_watchers(&thread_id, &envelope.event) {
                self.notify(&thread_id, &envelope.event);
            }
            return;
        }
        match envelope.event {
            AgentEvent::MessageStarted { message } if message.role == Role::Assistant => {
                for b in &chats {
                    if let Some(connector) = self.gateway.connector(b.platform) {
                        connector.typing(&b.chat);
                    }
                }
            }
            AgentEvent::TextDelta { .. } => {
                if let (Some(text), Some(progress)) = (writing, working.get_mut(&thread_id)) {
                    for b in &chats {
                        self.show_writing(b, progress, &text);
                    }
                }
            }
            AgentEvent::ToolStarted { .. } => {
                let progress = working.entry(thread_id).or_default();
                if progress.typed.is_some_and(|at| at.elapsed() < TYPING_EVERY) {
                    return;
                }
                progress.typed = Some(Instant::now());
                for b in &chats {
                    if let Some(connector) = self.gateway.connector(b.platform) {
                        connector.typing(&b.chat);
                    }
                }
            }
            AgentEvent::ApprovalRequested { approval, .. }
                if !approval.auto && approval.status == ApprovalStatus::Pending =>
            {
                let (text, buttons) = request(&approval);
                for b in &chats {
                    self.say_with(b.platform, &b.chat, &text, &buttons);
                }
            }
            AgentEvent::ApprovalResolved {
                approval_id,
                allowed,
                answers,
                auto: false,
                ..
            } => {
                let from = self.gateway.lock().answered.remove(&approval_id);
                for b in &chats {
                    let text = outcome(
                        allowed,
                        !answers.is_empty(),
                        from.as_ref(),
                        b.platform,
                        &b.chat,
                    );
                    self.say(b.platform, &b.chat, &text);
                }
            }
            AgentEvent::TurnCompleted {
                message_id,
                status,
                error,
            } => {
                // Its own conversations get the reply, so no other is still owed word of it.
                self.unwatch(&thread_id);
                self.chat_app_returns(&thread_id);
                let notes = working
                    .remove(&thread_id)
                    .map(|progress| progress.notes)
                    .unwrap_or_default();
                let Some(reply) = self.reply(&thread_id, &message_id, status, error) else {
                    return;
                };
                self.chat_app_used(&thread_id);
                for b in &chats {
                    let note = notes
                        .get(&(b.platform, b.chat.clone()))
                        .filter(|id| !id.is_empty());
                    if !note.is_some_and(|id| self.finish_writing(b, id, &reply)) {
                        self.say_long(b.platform, &b.chat, &reply);
                    }
                }
            }
            _ => {}
        }
    }

    /// Shows a message the person sent to `thread_id` in the chats that continue it, except the
    /// conversation `from` it was sent from.
    fn show_sent(&self, thread_id: &str, message: &Message, from: Option<&(Platform, String)>) {
        // What is typed on the computer stays there.
        if from.is_none() {
            return;
        }
        let text = sent(message);
        for b in self.bound_chats(thread_id) {
            if from.is_some_and(|(platform, chat)| *platform == b.platform && *chat == b.chat) {
                continue;
            }
            self.say(b.platform, &b.chat, &text);
        }
    }

    /// Shows the reply written so far in a chat, in one message it edits as more is written.
    /// Where a message cannot be edited, or each is mail or a paid text, the chat shows typing.
    fn show_writing(&self, chat: &Binding, progress: &mut Progress, text: &str) {
        let Some(connector) = self.gateway.connector(chat.platform) else {
            return;
        };
        let text = latest(text, chat.platform.limit());
        let key = (chat.platform, chat.chat.clone());
        match progress.notes.get(&key) {
            Some(id) if id.is_empty() => connector.typing(&chat.chat),
            Some(id) => {
                if let Err(error) = connector.edit(&chat.chat, id, &text) {
                    log::debug!(error:% = error; "could not update a reply in a chat app: {{error}}");
                }
            }
            None if !connector.notes() => {
                connector.typing(&chat.chat);
                progress.notes.insert(key, String::new());
            }
            None => {
                let id = connector
                    .send(&chat.chat, &text, &[])
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                progress.notes.insert(key, id);
            }
        }
    }

    /// Turns the message `id`, which showed the reply being written, into the finished `reply`,
    /// with what does not fit in it sent after. Returns whether the chat now has the reply.
    fn finish_writing(&self, chat: &Binding, id: &str, reply: &str) -> bool {
        let Some(connector) = self.gateway.connector(chat.platform) else {
            return false;
        };
        let pieces = chunks(reply, chat.platform.limit());
        let Some((first, rest)) = pieces.split_first() else {
            return false;
        };
        // A reply long enough to go as a file is sent whole, as one.
        if pieces.len() > MESSAGES || connector.edit(&chat.chat, id, first).is_err() {
            return false;
        }
        for piece in rest {
            self.say(chat.platform, &chat.chat, piece);
        }
        true
    }

    /// Whether the turn that writes the message `reply` of `thread_id` was asked for in a chat
    /// app: the message before it is the person's, sent from one.
    fn asked_on_chat_app(&self, thread_id: &str, reply: &str) -> bool {
        let Some(thread) = self.store.thread(thread_id) else {
            return false;
        };
        let before = match thread.messages.iter().position(|m| m.id == reply) {
            Some(at) => thread.messages[..at].last(),
            None => thread.messages.last(),
        };
        before.is_some_and(|m| m.role == Role::User && m.via.is_some())
    }

    /// What a chat app is told when a turn ends: the reply, what went wrong, and the files it
    /// changed.
    pub(super) fn reply(
        &self,
        thread_id: &str,
        message_id: &str,
        status: MessageStatus,
        error: Option<String>,
    ) -> Option<String> {
        let thread = self.store.thread(thread_id)?;
        let message = thread.messages.iter().find(|m| m.id == message_id)?;
        let mut parts = Vec::new();
        let text = message.text.trim();
        if !text.is_empty() {
            parts.push(text.to_string());
        }
        match status {
            MessageStatus::Streaming => return None,
            MessageStatus::Complete => {}
            MessageStatus::Error => parts.push(format!(
                "Error: {}",
                error.or_else(|| message.error.clone()).unwrap_or_default()
            )),
            MessageStatus::Cancelled => parts.push("Stopped.".to_string()),
            MessageStatus::Interrupted => parts.push("Interrupted.".to_string()),
        }
        if !message.files.is_empty() {
            let mut files: Vec<_> = message.files.iter().take(FILES_LISTED).cloned().collect();
            if message.files.len() > FILES_LISTED {
                files.push(format!("and {} more", message.files.len() - FILES_LISTED));
            }
            parts.push(format!("Changed: {}", files.join(", ")));
        }
        if parts.is_empty() {
            parts.push("Done.".to_string());
        }
        Some(parts.join("\n\n"))
    }
}

/// What a chat app is shown of the agent's request, and the buttons that answer it.
pub(super) fn request(approval: &Approval) -> (String, Vec<Button>) {
    if approval.secret.is_some() {
        let text = format!(
            "The agent asks for a secret: {}. Enter it in Patchwork on your computer.",
            approval.title
        );
        return (text, Vec::new());
    }
    if approval.questions.is_empty() {
        let mut text = approval.title.clone();
        if !approval.detail.trim().is_empty() {
            text.push_str("\n\n");
            text.push_str(&clip(&approval.detail, DETAIL_LIMIT));
        }
        let mut answers = vec![("Allow", "allow"), ("Deny", "deny")];
        if savable(approval) {
            text.push_str("\n\nReply allow, always or deny.");
            answers.insert(1, ("Always allow", "always"));
        } else {
            text.push_str("\n\nReply allow or deny.");
        }
        let buttons = answers
            .into_iter()
            .map(|(label, reply)| Button {
                label: label.to_string(),
                reply: reply.to_string(),
            })
            .collect();
        return (text, buttons);
    }
    let mut blocks = Vec::new();
    for q in &approval.questions {
        let mut block = if q.header.trim().is_empty() {
            q.question.clone()
        } else {
            format!("{}: {}", q.header, q.question)
        };
        for (i, option) in q.options.iter().enumerate() {
            let _ = write!(block, "\n{}. {}", i + 1, option.label);
            if !option.description.trim().is_empty() {
                let _ = write!(block, ": {}", option.description);
            }
        }
        blocks.push(block);
    }
    let how = if approval.questions.len() > 1 {
        "Reply with one line per question: the numbers of your answers, or your answer in words."
    } else if approval.questions.iter().any(|q| q.multi_select) {
        "Reply with the numbers of your answers, separated by commas, or your answer in words."
    } else {
        "Reply with the number of your answer, or your answer in words."
    };
    blocks.push(how.to_string());
    let buttons = match approval.questions.as_slice() {
        [q] if !q.multi_select => q
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| Button {
                label: o.label.clone(),
                reply: (i + 1).to_string(),
            })
            .collect(),
        _ => Vec::new(),
    };
    (blocks.join("\n\n"), buttons)
}

/// Whether "Always allow" can save rules that answer requests like `approval` from now on.
pub(super) fn savable(approval: &Approval) -> bool {
    approval.questions.is_empty() && approval.kind.is_some() && !approval.suggestions.is_empty()
}

/// What `chat` on `platform` is told once a request was answered: just that when it was
/// answered there, else also where it was answered.
pub(super) fn outcome(
    allowed: bool,
    answered: bool,
    from: Option<&(Platform, String)>,
    platform: Platform,
    chat: &str,
) -> String {
    let what = if answered {
        "Answered"
    } else if allowed {
        "Allowed"
    } else {
        "Denied"
    };
    match from {
        Some((p, c)) if *p == platform && c == chat => format!("{what}."),
        Some((p, _)) => format!("{what} on {}.", p.label()),
        None => format!("{what} on your computer."),
    }
}

/// What the chats that continue a thread are shown of a message the person sent it: where it was
/// sent from, whether it steers or waits for the running reply, and the files it carries.
fn sent(message: &Message) -> String {
    let place = message.via.map_or("your computer", Platform::label);
    let how = if message.steer {
        ", steering"
    } else if message.queued {
        ", queued"
    } else {
        ""
    };
    let mut text = format!("You, on {place}{how}:");
    let said = message.text.trim();
    if !said.is_empty() {
        text.push('\n');
        text.push_str(said);
    }
    if !message.attachments.is_empty() {
        let names: Vec<&str> = message
            .attachments
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        let _ = write!(text, "\nAttached: {}", names.join(", "));
    }
    text
}

/// The last `limit` characters of `text`, for a reply longer than a message holds.
fn latest(text: &str, limit: usize) -> String {
    let text = text.trim();
    let over = text.chars().count().saturating_sub(limit.saturating_sub(1));
    match text.char_indices().nth(over) {
        Some((at, _)) if over > 0 => format!("\u{2026}{}", &text[at..]),
        _ => text.to_string(),
    }
}
