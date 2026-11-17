//! What the task list shows of the chat apps on each thread now, and the
//! conversations a thread's notices go to.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use super::platform::Platform;
use super::types::{ChatApps, ChatPlace, Status};
use crate::Core;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::system::now_ms;
use crate::threads::chat_tools::last_active;
use crate::threads::{Standing, Thread};

/// The most chats a turn is told about.
const RECENT: usize = 8;

/// How long a chat that is neither replying nor waiting stays in what a turn is told, in
/// milliseconds.
const RECENT_MS: u64 = 24 * 60 * 60 * 1000;

/// How long the chat an agent last worked with is still what "it" means, in milliseconds.
const SUBJECT_MS: u64 = 30 * 60 * 1000;

impl Core {
    /// The chat apps of every thread that has one on it, by thread id.
    ///
    /// A conversation is on the thread it talks to, or, while that thread's agent works with
    /// another chat, on that chat.
    #[must_use]
    pub fn thread_chat_apps(&self) -> HashMap<String, ChatApps> {
        let mut apps: HashMap<String, ChatApps> = HashMap::new();
        let (visiting, used) = {
            let state = self.gateway.lock();
            (state.visiting.clone(), state.used.clone())
        };
        for binding in self.bindings() {
            let shown = visiting
                .get(&binding.thread_id)
                .unwrap_or(&binding.thread_id)
                .clone();
            let apps = apps.entry(shown).or_default();
            apps.used_at = apps.used_at.max(used.get(&binding.thread_id).copied());
            apps.chats.push(binding);
        }
        apps
    }

    /// Notes that the agent of `caller` works with the chat `target`, so the task list shows
    /// the caller's chat apps on that chat until the turn ends.
    pub(crate) fn chat_app_visits(&self, caller: &str, target: &str) {
        if self.bound_chats(caller).is_empty() {
            return;
        }
        let before = {
            let mut state = self.gateway.lock();
            state
                .subject
                .insert(caller.to_string(), (target.to_string(), now_ms()));
            state
                .visiting
                .insert(caller.to_string(), target.to_string())
        };
        if before.as_deref() == Some(target) {
            return;
        }
        self.chat_apps_changed(&[Some(caller), before.as_deref(), Some(target)]);
    }

    /// `text` with a note after it of what the other chats are doing now and which one this
    /// thread's agent last worked with, for a thread a chat app talks to.
    ///
    /// It goes with every message, so the agent picks a loosely named chat from what is there
    /// and knows what "it" means without being told a name.
    pub(crate) fn chat_app_context(&self, thread_id: &str, text: String) -> String {
        if self.bound_chats(thread_id).is_empty() {
            return text;
        }
        let busy = |t: &Thread| self.runner.is_starting(&t.id) || self.runner.is_running(&t.id);
        let mut chats: Vec<Thread> = self
            .store
            .threads()
            .into_iter()
            .filter(|t| t.id != thread_id && t.parent.is_none() && t.archived_at.is_none())
            .collect();
        if chats.is_empty() {
            return text;
        }
        chats.sort_by_key(|t| std::cmp::Reverse((busy(t), last_active(t))));
        let now = now_ms();
        let line = |t: &Thread| {
            let state = match t.standing() {
                Standing::Waiting => "waiting for an answer",
                _ if busy(t) => "replying",
                Standing::Working => "replying",
                Standing::Failed => "failed",
                Standing::Done => "done",
            };
            format!(
                "{}, {state}, {} (chat_id {})",
                self.heading(t),
                ago(now.saturating_sub(last_active(t))),
                t.id
            )
        };
        // A chat worked with a while ago is no longer what "it" means, so a late "stop that"
        // has to name its chat.
        let subject = self.gateway.lock().subject.get(thread_id).cloned();
        let last = subject
            .filter(|(_, at)| now.saturating_sub(*at) < SUBJECT_MS)
            .and_then(|(id, _)| chats.iter().find(|t| t.id == id))
            .map(|t| {
                format!(
                    "The chat you last worked with here: {}. A request that says it or that, \
                     or names no chat, means this one.\n",
                    line(t)
                )
            })
            .unwrap_or_default();
        let rows: Vec<String> = chats
            .iter()
            .filter(|t| {
                busy(t)
                    || t.standing() == Standing::Waiting
                    || now.saturating_sub(last_active(t)) < RECENT_MS
            })
            .take(RECENT)
            .map(|t| format!("- {}", line(t)))
            .collect();
        format!(
            "{text}\n\n<patchwork-now>\nReply in about 250 characters unless more is vital. \
             Patchwork's chats of the last day, the ones replying first; list_chats has the \
             rest. When the person names one loosely, pick it from here and use its chat_id; \
             ask which only when two fit equally well or none is named.\n{last}{}\n\
             </patchwork-now>",
            rows.join("\n")
        )
    }

    /// Puts the chat apps of `caller` on its own row again, once its turn has ended.
    pub(super) fn chat_app_returns(&self, caller: &str) {
        let before = self.gateway.lock().visiting.remove(caller);
        if let Some(before) = before {
            self.chat_apps_changed(&[Some(caller), Some(&before)]);
        }
    }

    /// Notes that the conversations of `thread_id` asked for a reply or got one just now.
    pub(super) fn chat_app_used(&self, thread_id: &str) {
        self.gateway
            .lock()
            .used
            .insert(thread_id.to_string(), now_ms());
        self.chat_apps_changed(&[Some(thread_id)]);
    }

    /// Tells the window that the chat apps of `threads` changed.
    fn chat_apps_changed(&self, threads: &[Option<&str>]) {
        for thread_id in threads.iter().flatten() {
            self.emit(AgentEventEnvelope {
                thread_id: (*thread_id).to_string(),
                event: AgentEvent::ChatAppsChanged,
            });
        }
    }

    /// The conversations notices go to: each pairing's latest one, on a chat app that is
    /// connected now.
    #[must_use]
    pub fn chat_places(&self) -> Vec<ChatPlace> {
        let connected: Vec<Platform> = self
            .gateway
            .lock()
            .statuses
            .iter()
            .filter(|(_, status)| matches!(status, Status::Connected(_)))
            .map(|(platform, _)| *platform)
            .collect();
        let mut places: Vec<ChatPlace> = Vec::new();
        for paired in self.pairings() {
            let Some(chat) = paired.chat else { continue };
            let known = places
                .iter()
                .any(|p| p.platform == paired.platform && p.chat == chat);
            if connected.contains(&paired.platform) && !known {
                places.push(ChatPlace {
                    platform: paired.platform,
                    chat,
                    name: paired.name,
                });
            }
        }
        places.sort_by_key(|p| p.platform);
        places
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

/// How long ago something happened, given the milliseconds since, as "5 min ago".
fn ago(ms: u64) -> String {
    let minutes = ms / 60_000;
    match minutes {
        0 => "just now".to_string(),
        1..60 => format!("{minutes} min ago"),
        60..1440 => format!("{} h ago", minutes / 60),
        _ => format!("{} d ago", minutes / 1440),
    }
}
