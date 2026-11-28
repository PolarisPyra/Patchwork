//! What goes back out to chat apps: a chat's reply as it is written, the agent's requests, and
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
use patchwork_core::Core;
use patchwork_core::provider::clip;
use patchwork_core::events::{ChatEvent, ChatEventEnvelope};
use patchwork_core::chats::{Approval, ApprovalStatus, Message, MessageStatus, Role};
#[allow(
    unused_imports,
    reason = "each file takes the core's gateway methods it calls"
)]
use crate::prelude::*;
use crate::hub::HUB;

/// How often the reply a chat app shows being written changes at most.
const REPLY_EVERY: Duration = Duration::from_millis(1500);

/// How often a working chat shows typing at most.
const TYPING_EVERY: Duration = Duration::from_secs(5);

/// The most characters of a tool's summary or a request's detail a chat app is shown.
const DETAIL_LIMIT: usize = 1500;

/// The most files a reply lists as changed.
const FILES_LISTED: usize = 10;

/// What a chat app was shown of a turn that is working.
#[derive(Default)]
pub(crate) struct Progress {
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

/// Sends the events of bound chats, and the messages the person sends them, to their chats
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
                chat_id,
                message,
                from,
            } => {
                // A message from a chat app joins a turn asked for on the computer, so the
                // reply goes there too.
                if let (Some(_), Some(progress)) = (&from, working.get_mut(&chat_id)) {
                    progress.local = false;
                }
                core.show_sent(&chat_id, &message, from.as_ref());
            }
        }
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
    conversation: &str,
) -> String {
    let what = if answered {
        "Answered"
    } else if allowed {
        "Allowed"
    } else {
        "Denied"
    };
    match from {
        Some((p, c)) if *p == platform && c == conversation => format!("{what}."),
        Some((p, _)) => format!("{what} on {}.", p.label()),
        None => format!("{what} on your computer."),
    }
}

/// What the chats that continue a chat are shown of a message the person sent it: where it was
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

/// The core's outbound methods the gateway calls.
pub(crate) trait Outbound {
    /// Sends `text` to `chat`, split to the platform's length.
    fn say(&self, platform: Platform, conversation: &str, text: &str);

    /// Sends `text` to `chat` with `buttons` under its last piece, and returns the id of that
    /// piece when it can be edited.
    fn say_with(
        &self,
        platform: Platform,
        conversation: &str,
        text: &str,
        buttons: &[Button],
    ) -> Option<String>;

    fn send_out(&self, envelope: ChatEventEnvelope, working: &mut HashMap<String, Progress>);

    /// Shows a message the person sent to `chat_id` in the chats that continue it, except the
    /// conversation `from` it was sent from.
    fn show_sent(&self, chat_id: &str, message: &Message, from: Option<&(Platform, String)>);

    /// Shows the reply written so far in a chat, in one message it edits as more is written.
    /// Where a message cannot be edited, or each is mail or a paid text, the chat shows typing.
    fn show_writing(&self, conversation: &Binding, progress: &mut Progress, text: &str);

    /// Turns the message `id`, which showed the reply being written, into the finished `reply`,
    /// with what does not fit in it sent after. Returns whether the chat now has the reply.
    fn finish_writing(&self, conversation: &Binding, id: &str, reply: &str) -> bool;

    /// Whether the turn that writes the message `reply` of `chat_id` was asked for in a chat
    /// app: the message before it is the person's, sent from one.
    fn asked_on_chat_app(&self, chat_id: &str, reply: &str) -> bool;

    /// What a chat app is told when a turn ends: the reply, what went wrong, and the files it
    /// changed.
    fn reply(
        &self,
        chat_id: &str,
        message_id: &str,
        status: MessageStatus,
        error: Option<String>,
    ) -> Option<String>;
}

impl Outbound for Core {
    fn say(&self, platform: Platform, conversation: &str, text: &str) {
        self.say_with(platform, conversation, text, &[]);
    }

    fn say_with(
        &self,
        platform: Platform,
        conversation: &str,
        text: &str,
        buttons: &[Button],
    ) -> Option<String> {
        let connector = HUB.connector(platform)?;
        let pieces = chunks(text, platform.limit());
        let last = pieces.len().saturating_sub(1);
        let mut id = None;
        for (i, piece) in pieces.iter().enumerate() {
            let buttons = if i == last { buttons } else { &[] };
            match connector.send(conversation, piece, buttons) {
                Ok(sent) => id = sent,
                Err(error) => {
                    log::warn!(platform = platform.id(), error:% = error; "could not send to {{platform}}: {{error}}");
                    return None;
                }
            }
        }
        id
    }

    fn send_out(&self, envelope: ChatEventEnvelope, working: &mut HashMap<String, Progress>) {
        let chat_id = envelope.chat_id;
        // A chat that works again, or is gone, leaves no usage notice to continue it.
        if matches!(&envelope.event, ChatEvent::MessageStarted { message } if message.role == Role::Assistant)
            || matches!(envelope.event, ChatEvent::ChatDeleted)
        {
            self.drop_reset_notices(&chat_id);
        }
        if matches!(envelope.event, ChatEvent::ChatDeleted) {
            working.remove(&chat_id);
            self.unbind_chat(&chat_id);
            return;
        }
        // A reply is written in many small pieces, so most are only kept.
        let mut writing = None;
        if let ChatEvent::TextDelta { text, .. } = &envelope.event {
            let Some(progress) = working.get_mut(&chat_id).filter(|p| !p.local) else {
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
        let mut conversations = self.bound_conversations(&chat_id);
        if let ChatEvent::MessageStarted { message } = &envelope.event
            && message.role == Role::Assistant
            && !conversations.is_empty()
        {
            let local = !self.asked_on_chat_app(&chat_id, &message.id);
            if !local {
                self.chat_app_used(&chat_id);
            }
            let progress = Progress {
                local,
                ..Progress::default()
            };
            working.insert(chat_id.clone(), progress);
        }
        // A turn asked for on the computer is answered there. Its conversations hear of it only
        // as any other chat's would, by the notification settings.
        if working.get(&chat_id).is_some_and(|p| p.local) {
            if matches!(envelope.event, ChatEvent::TurnCompleted { .. }) {
                working.remove(&chat_id);
                self.chat_app_returns(&chat_id);
            }
            conversations.clear();
        }
        if conversations.is_empty() {
            // A chat a dispatcher's conversation watches is told there; the rest go by the
            // notification settings.
            if !self.tell_watchers(&chat_id, &envelope.event) {
                self.notify(&chat_id, &envelope.event);
            }
            return;
        }
        match envelope.event {
            ChatEvent::MessageStarted { message } if message.role == Role::Assistant => {
                for b in &conversations {
                    if let Some(connector) = HUB.connector(b.platform) {
                        connector.typing(&b.conversation);
                    }
                }
            }
            ChatEvent::TextDelta { .. } => {
                if let (Some(text), Some(progress)) = (writing, working.get_mut(&chat_id)) {
                    for b in &conversations {
                        self.show_writing(b, progress, &text);
                    }
                }
            }
            ChatEvent::ToolStarted { .. } => {
                let progress = working.entry(chat_id).or_default();
                if progress.typed.is_some_and(|at| at.elapsed() < TYPING_EVERY) {
                    return;
                }
                progress.typed = Some(Instant::now());
                for b in &conversations {
                    if let Some(connector) = HUB.connector(b.platform) {
                        connector.typing(&b.conversation);
                    }
                }
            }
            ChatEvent::ApprovalRequested { approval, .. }
                if !approval.auto && approval.status == ApprovalStatus::Pending =>
            {
                let (text, buttons) = request(&approval);
                for b in &conversations {
                    self.say_with(b.platform, &b.conversation, &text, &buttons);
                }
            }
            ChatEvent::ApprovalResolved {
                approval_id,
                allowed,
                answers,
                auto: false,
                ..
            } => {
                let from = HUB.lock().answered.remove(&approval_id);
                for b in &conversations {
                    let text = outcome(
                        allowed,
                        !answers.is_empty(),
                        from.as_ref(),
                        b.platform,
                        &b.conversation,
                    );
                    self.say(b.platform, &b.conversation, &text);
                }
            }
            ChatEvent::TurnCompleted {
                message_id,
                status,
                error,
            } => {
                // Its own conversations get the reply, so no other is still owed word of it.
                self.unwatch(&chat_id);
                self.chat_app_returns(&chat_id);
                let notes = working
                    .remove(&chat_id)
                    .map(|progress| progress.notes)
                    .unwrap_or_default();
                let Some(reply) = self.reply(&chat_id, &message_id, status, error) else {
                    return;
                };
                self.chat_app_used(&chat_id);
                for b in &conversations {
                    let note = notes
                        .get(&(b.platform, b.conversation.clone()))
                        .filter(|id| !id.is_empty());
                    if !note.is_some_and(|id| self.finish_writing(b, id, &reply)) {
                        self.say_long(b.platform, &b.conversation, &reply);
                    }
                }
            }
            _ => {}
        }
    }

    fn show_sent(&self, chat_id: &str, message: &Message, from: Option<&(Platform, String)>) {
        // What is typed on the computer stays there.
        if from.is_none() {
            return;
        }
        let text = sent(message);
        for b in self.bound_conversations(chat_id) {
            if from.is_some_and(|(platform, conversation)| {
                *platform == b.platform && *conversation == b.conversation
            }) {
                continue;
            }
            self.say(b.platform, &b.conversation, &text);
        }
    }

    fn show_writing(&self, conversation: &Binding, progress: &mut Progress, text: &str) {
        let Some(connector) = HUB.connector(conversation.platform) else {
            return;
        };
        let text = latest(text, conversation.platform.limit());
        let key = (conversation.platform, conversation.conversation.clone());
        match progress.notes.get(&key) {
            Some(id) if id.is_empty() => connector.typing(&conversation.conversation),
            Some(id) => {
                if let Err(error) = connector.edit(&conversation.conversation, id, &text) {
                    log::debug!(error:% = error; "could not update a reply in a chat app: {{error}}");
                }
            }
            None if !connector.notes() => {
                connector.typing(&conversation.conversation);
                progress.notes.insert(key, String::new());
            }
            None => {
                let id = connector
                    .send(&conversation.conversation, &text, &[])
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                progress.notes.insert(key, id);
            }
        }
    }

    fn finish_writing(&self, conversation: &Binding, id: &str, reply: &str) -> bool {
        let Some(connector) = HUB.connector(conversation.platform) else {
            return false;
        };
        let pieces = chunks(reply, conversation.platform.limit());
        let Some((first, rest)) = pieces.split_first() else {
            return false;
        };
        // A reply long enough to go as a file is sent whole, as one.
        if pieces.len() > MESSAGES
            || connector
                .edit(&conversation.conversation, id, first)
                .is_err()
        {
            return false;
        }
        for piece in rest {
            self.say(conversation.platform, &conversation.conversation, piece);
        }
        true
    }

    fn asked_on_chat_app(&self, chat_id: &str, reply: &str) -> bool {
        let Some(chat) = self.store().chat(chat_id) else {
            return false;
        };
        let before = match chat.messages.iter().position(|m| m.id == reply) {
            Some(at) => chat.messages[..at].last(),
            None => chat.messages.last(),
        };
        before.is_some_and(|m| m.role == Role::User && m.via.is_some())
    }

    fn reply(
        &self,
        chat_id: &str,
        message_id: &str,
        status: MessageStatus,
        error: Option<String>,
    ) -> Option<String> {
        let chat = self.store().chat(chat_id)?;
        let message = chat.messages.iter().find(|m| m.id == message_id)?;
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
