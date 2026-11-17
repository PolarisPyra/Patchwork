//! Telling the pairings when a chat no conversation continues finishes, meets its goal,
//! fails or asks something,
//! and answering what it asks from the conversation they were told in.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use super::outbound::{clip, outcome};
use super::platform::Platform;
use super::types::Button;
use crate::Core;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::notifications::{ChatAppsWhen, Kind, Notices, Place, resets_in};
use crate::system::now_ms;
use crate::threads::{Approval, ApprovalStatus, Thread};

/// The most characters of a finished chat's reply a notice shows.
const REPLY_LIMIT: usize = 300;

/// A request sent to a conversation on a chat app, which it can answer until it is resolved.
pub(super) struct Notice {
    pub platform: Platform,
    pub chat: String,
    pub thread_id: String,
    pub approval_id: String,
}

impl Core {
    /// Tells the pairings about `event` of a thread no conversation continues, when the
    /// settings ask for it there and, unless they say always, you are away.
    pub(super) fn notify(&self, thread_id: &str, event: &AgentEvent) {
        if let AgentEvent::ApprovalResolved {
            approval_id,
            allowed,
            answers,
            auto: false,
            ..
        } = event
        {
            self.close_notices(approval_id, *allowed, !answers.is_empty());
            return;
        }
        let relevant = matches!(
            event,
            AgentEvent::ApprovalRequested { .. }
                | AgentEvent::TurnCompleted { .. }
                | AgentEvent::GoalUpdated { goal: Some(_) }
        );
        let always = self.store.settings().notifications.chat_apps_when == ChatAppsWhen::Always;
        if !relevant || !(always || self.away()) {
            return;
        }
        let Some(thread) = self.store.thread(thread_id) else {
            return;
        };
        let Some(kind) = self.notice_kind(Place::ChatApps, &thread, event) else {
            return;
        };
        match event {
            AgentEvent::ApprovalRequested { approval, .. } => {
                for place in self.chat_places() {
                    self.show_request(&thread, approval, place.platform, &place.chat);
                }
            }
            AgentEvent::GoalUpdated { goal: Some(goal) } => {
                let text = format!(
                    "{} met its goal:\n\n{}",
                    self.heading(&thread),
                    clip(&goal.objective, REPLY_LIMIT)
                );
                self.tell_finished(&thread, &text);
            }
            AgentEvent::TurnCompleted { .. } if kind == Kind::Usage => {
                let resets = self
                    .limit_reset(&thread)
                    .map(|at| format!(" It resets {}.", resets_in(at, now_ms())))
                    .unwrap_or_default();
                let text = format!("{} ran out of usage.{resets}", self.heading(&thread));
                let resume = Button {
                    label: "Continue when it resets".to_string(),
                    reply: format!("/resume {thread_id}"),
                };
                for place in self.chat_places() {
                    self.say_with(
                        place.platform,
                        &place.chat,
                        &text,
                        std::slice::from_ref(&resume),
                    );
                }
            }
            AgentEvent::TurnCompleted {
                message_id,
                status,
                error,
            } => {
                let Some(reply) = self.reply(thread_id, message_id, *status, error.clone()) else {
                    return;
                };
                let how = if kind == Kind::Failed {
                    "stopped"
                } else {
                    "finished"
                };
                let text = format!(
                    "{} {how}:\n\n{}",
                    self.heading(&thread),
                    clip(&reply, REPLY_LIMIT)
                );
                self.tell_finished(&thread, &text);
            }
            _ => {}
        }
    }

    /// Sends `text` to the pairings, with buttons to mute the chat.
    fn tell_finished(&self, thread: &Thread, text: &str) {
        let mut buttons = vec![Button {
            label: "Mute this chat".to_string(),
            reply: format!("/mute {}", thread.id),
        }];
        if let Some(project) = &thread.project_id {
            buttons.push(Button {
                label: "Mute project".to_string(),
                reply: format!("/mute project {project}"),
            });
        }
        for place in self.chat_places() {
            self.say_with(place.platform, &place.chat, text, &buttons);
        }
    }

    /// Answers the request a conversation was told about last with `text`, and says whether
    /// there was one still waiting.
    pub(super) fn answer_notice(
        self: &Arc<Self>,
        platform: Platform,
        chat: &str,
        text: &str,
    ) -> bool {
        let told: Vec<(String, String)> = self
            .gateway
            .lock()
            .notices
            .iter()
            .rev()
            .filter(|n| n.platform == platform && n.chat == chat)
            .map(|n| (n.thread_id.clone(), n.approval_id.clone()))
            .collect();
        for (thread_id, approval_id) in told {
            match self.approval_waiting(&thread_id, &approval_id) {
                Some(approval) => {
                    self.answer(platform, chat, &thread_id, &approval, text);
                    return true;
                }
                // A request its turn left unanswered is no longer waiting.
                None => self
                    .gateway
                    .lock()
                    .notices
                    .retain(|n| n.approval_id != approval_id),
            }
        }
        false
    }

    /// Answers the request `approval_id` with `text`, from a notice's button.
    pub(super) fn answer_by_id(
        self: &Arc<Self>,
        platform: Platform,
        chat: &str,
        approval_id: &str,
        text: &str,
    ) {
        let thread_id = self.store.with_threads(|threads| {
            threads
                .iter()
                .find(|t| {
                    t.messages
                        .iter()
                        .any(|m| m.approvals.iter().any(|a| a.id == approval_id))
                })
                .map(|t| t.id.clone())
        });
        match thread_id.and_then(|id| Some((self.approval_waiting(&id, approval_id)?, id))) {
            Some((approval, id)) => self.answer(platform, chat, &id, &approval, text),
            None => self.say(platform, chat, "That request is no longer waiting."),
        }
    }

    /// The request `approval_id` of `thread_id`, while its agent waits on it.
    fn approval_waiting(&self, thread_id: &str, approval_id: &str) -> Option<Approval> {
        if !self.runner.is_running(thread_id) {
            return None;
        }
        self.store
            .thread(thread_id)?
            .messages
            .iter()
            .flat_map(|m| m.approvals.iter())
            .find(|a| a.id == approval_id && a.status == ApprovalStatus::Pending)
            .cloned()
    }

    /// Forgets the notices of a request that was answered, and tells their conversations how.
    pub(super) fn close_notices(&self, approval_id: &str, allowed: bool, answered: bool) {
        let (closed, from) = {
            let mut state = self.gateway.lock();
            let (closed, kept) = std::mem::take(&mut state.notices)
                .into_iter()
                .partition::<Vec<_>, _>(|n| n.approval_id == approval_id);
            state.notices = kept;
            let from = if closed.is_empty() {
                None
            } else {
                state.answered.remove(approval_id)
            };
            (closed, from)
        };
        for n in closed {
            let text = outcome(allowed, answered, from.as_ref(), n.platform, &n.chat);
            self.say(n.platform, &n.chat, &text);
        }
    }

    /// Stops the notices of the chat `rest` names, or of every chat in a project with
    /// `project <id>`, and returns what to tell the conversation.
    pub(super) fn mute_notices(&self, rest: &str) -> String {
        let (project, id) = match rest.split_once(char::is_whitespace) {
            Some(("project", id)) => (true, id.trim()),
            _ => (false, rest),
        };
        let named = if project {
            self.store
                .settings()
                .projects
                .into_iter()
                .find(|p| p.id == id)
                .map(|p| format!("chats in {}", p.name))
        } else {
            self.store
                .thread(id)
                .map(|t| format!("\u{201c}{}\u{201d}", t.title))
        };
        let Some(named) = named else {
            return "Press a Mute button under a notice to stop hearing from that chat."
                .to_string();
        };
        let saved = if project {
            self.set_project_notices(id, false)
        } else {
            self.set_thread_notices(id, Some(Notices::Off))
        };
        match saved {
            Ok(()) => {
                format!("No more notices from {named}. Ask to hear from it again to undo this.")
            }
            Err(error) => format!("Patchwork could not mute that: {error}"),
        }
    }

    /// Sets the chat `thread_id` to continue once its agent's usage limit resets, from a
    /// notice's button, and returns what to tell the conversation.
    pub(super) fn resume_at_reset(&self, thread_id: &str) -> String {
        let Some(thread) = self.store.thread(thread_id) else {
            return "Press Continue when it resets under a notice to pick the chat.".to_string();
        };
        match self.resume_at_limit_reset(thread_id) {
            Ok(at) => {
                self.emit(AgentEventEnvelope {
                    thread_id: thread_id.to_string(),
                    event: AgentEvent::OptionsChanged,
                });
                format!(
                    "\u{201c}{}\u{201d} continues {}.",
                    thread.title,
                    resets_in(at, now_ms())
                )
            }
            Err(error) => format!("Patchwork could not set that: {error}"),
        }
    }

    /// The thread's title in quotes, and its project when it has one.
    pub(super) fn heading(&self, thread: &Thread) -> String {
        let project = thread.project_id.as_ref().and_then(|id| {
            self.store
                .settings()
                .projects
                .into_iter()
                .find(|p| &p.id == id)
                .map(|p| p.name)
        });
        match project {
            Some(name) => format!("\u{201c}{}\u{201d} in {name}", thread.title),
            None => format!("\u{201c}{}\u{201d}", thread.title),
        }
    }
}
