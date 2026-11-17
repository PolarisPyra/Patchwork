//! Continuing a thread on a chat app from the computer, and what the task list shows of the
//! chat apps each thread came from and is continued on.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use super::outbound::clip;
use super::platform::Platform;
use super::types::{ChatApps, ChatPlace, Status};
use crate::threads::{MessageStatus, Role, Thread};
use crate::{Error, Core};

/// The most characters of the last reply a chat is shown when a thread is continued there.
const LAST_REPLY_LIMIT: usize = 1500;

impl Core {
    /// The chat apps of every thread that came from one or is continued on one, by thread id.
    #[must_use]
    pub fn thread_chat_apps(&self) -> HashMap<String, ChatApps> {
        let mut apps: HashMap<String, ChatApps> = HashMap::new();
        self.store.with_threads(|threads| {
            for thread in threads {
                if let Some(platform) = started_on(thread) {
                    apps.entry(thread.id.clone()).or_default().started_on = Some(platform);
                }
            }
        });
        for binding in self.bindings() {
            apps.entry(binding.thread_id.clone())
                .or_default()
                .chats
                .push(binding);
        }
        apps
    }

    /// The conversations a thread can be continued in: each paired person's latest one, on a
    /// chat app that is connected now.
    #[must_use]
    pub fn gateway_places(&self) -> Vec<ChatPlace> {
        let connected: Vec<Platform> = self
            .gateway
            .lock()
            .statuses
            .iter()
            .filter(|(_, status)| matches!(status, Status::Connected(_)))
            .map(|(platform, _)| *platform)
            .collect();
        let mut places: Vec<ChatPlace> = Vec::new();
        for user in self.gateway_users() {
            let Some(chat) = user.chat else { continue };
            let known = places
                .iter()
                .any(|p| p.platform == user.platform && p.chat == chat);
            if connected.contains(&user.platform) && !known {
                places.push(ChatPlace {
                    platform: user.platform,
                    chat,
                    name: user.name,
                });
            }
        }
        places.sort_by_key(|p| p.platform);
        places
    }

    /// Makes `chat` on `platform` talk to `thread_id`, and tells it so with the thread's last
    /// reply.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is gone or the chat app is not connected.
    pub fn continue_on_chat(
        &self,
        thread_id: &str,
        platform: Platform,
        chat: &str,
    ) -> Result<(), Error> {
        let thread = self
            .store
            .thread(thread_id)
            .ok_or(crate::threads::NOT_FOUND)?;
        if self.gateway.connector(platform).is_none() {
            return Err(format!("{} is not connected", platform.label()).into());
        }
        self.bind_chat(platform, chat, Some(thread_id));
        let mut text = format!(
            "Continuing \u{201c}{}\u{201d} here. What you send here goes to it, and what you type on your computer shows here too.",
            thread.title
        );
        if let Some(reply) = last_reply(&thread) {
            text.push_str("\n\nLast reply:\n");
            text.push_str(&clip(reply, LAST_REPLY_LIMIT));
        }
        self.say(platform, chat, &text);
        Ok(())
    }

    /// Stops `chat` on `platform` talking to its thread, and tells it so.
    pub fn stop_continuing_on_chat(&self, platform: Platform, chat: &str) {
        let title = self
            .bound_thread(platform, chat)
            .and_then(|id| self.store.thread(&id))
            .map(|t| t.title);
        self.bind_chat(platform, chat, None);
        if let Some(title) = title {
            self.say(
                platform,
                chat,
                &format!(
                    "Stopped continuing \u{201c}{title}\u{201d} here. Send a message to start a new chat."
                ),
            );
        }
    }
}

/// The chat app the thread's first message was sent from.
fn started_on(thread: &Thread) -> Option<Platform> {
    thread
        .messages
        .iter()
        .find(|m| m.role == Role::User)
        .and_then(|m| m.via)
}

/// The text of the thread's last finished reply, if it has one.
fn last_reply(thread: &Thread) -> Option<&str> {
    thread
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::Assistant && m.status == MessageStatus::Complete)
        .map(|m| m.text.trim())
        .filter(|text| !text.is_empty())
}
