//! Telling the pairings when a chat no conversation continues finishes, meets its goal,
//! fails or asks something,
//! and answering what it asks from the conversation they were told in.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::outbound::outcome;
use patchwork_core::provider::clip;
use super::platform::Platform;
use super::types::Button;
use patchwork_core::Core;
use patchwork_core::events::{ChatEvent, ChatEventEnvelope};
use patchwork_core::notifications::{ChatAppsWhen, Kind, Notices, Place, resets_in};
use patchwork_platform::now_ms;
use patchwork_core::chats::{Approval, ApprovalStatus, Chat};
#[allow(
    unused_imports,
    reason = "each file takes the core's gateway methods it calls"
)]
use crate::prelude::*;
use crate::hub::HUB;

/// The most characters of a finished chat's reply a notice shows.
const REPLY_LIMIT: usize = 300;

/// A request sent to a conversation on a chat app, which it can answer until it is resolved.
pub(super) struct Notice {
    pub platform: Platform,
    pub conversation: String,
    pub chat_id: String,
    pub approval_id: String,
}

/// The app state key of the usage notices still showing a Continue when it resets button.
const RESETS: &str = "gateway.resets";

/// A usage notice sent to a conversation, deleted once the chat is set to continue, starts
/// another turn, or is deleted, so no stale button is left to press.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResetNotice {
    pub platform: Platform,
    pub conversation: String,
    pub chat_id: String,
    pub message_id: String,
}

/// The core's notify methods the gateway calls.
pub(crate) trait Notify {
    /// Tells the pairings about `event` of a chat no conversation continues, when the
    /// settings ask for it there and, unless they say always, you are away.
    fn notify(&self, chat_id: &str, event: &ChatEvent);

    /// Sends `text` to the pairings, with buttons to mute the chat.
    fn tell_finished(&self, chat: &Chat, text: &str);

    /// Answers the request a conversation was told about last with `text`, and says whether
    /// there was one still waiting.
    fn answer_notice(self: &Arc<Self>, platform: Platform, conversation: &str, text: &str) -> bool;

    /// Answers the request `approval_id` with `text`, from a notice's button.
    fn answer_by_id(
        self: &Arc<Self>,
        platform: Platform,
        conversation: &str,
        approval_id: &str,
        text: &str,
    );

    /// The request `approval_id` of `chat_id`, while its agent waits on it.
    fn approval_waiting(&self, chat_id: &str, approval_id: &str) -> Option<Approval>;

    /// Forgets the notices of a request that was answered, and tells their conversations how.
    fn close_notices(&self, approval_id: &str, allowed: bool, answered: bool);

    /// Stops the notices of the chat `rest` names, or of every chat in a project with
    /// `project <id>`, and returns what to tell the conversation.
    fn mute_notices(&self, rest: &str) -> String;

    /// Sets the chat `chat_id` to continue once its agent's usage limit resets, from a
    /// notice's button, and returns what to tell the conversation.
    fn resume_at_reset(&self, chat_id: &str) -> String;

    /// Deletes the usage notices of `chat_id` from the chat apps, on a thread of its own, so a
    /// chat that has moved on shows no button to set it to continue.
    fn drop_reset_notices(&self, chat_id: &str);

    /// The chat's title in quotes, and its project when it has one.
    fn heading(&self, chat: &Chat) -> String;
}

impl Notify for Core {
    fn notify(&self, chat_id: &str, event: &ChatEvent) {
        if let ChatEvent::ApprovalResolved {
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
            ChatEvent::ApprovalRequested { .. }
                | ChatEvent::TurnCompleted { .. }
                | ChatEvent::GoalUpdated { goal: Some(_) }
        );
        let always = self.store().settings().notifications.chat_apps_when == ChatAppsWhen::Always;
        if !relevant || !(always || self.away()) {
            return;
        }
        let Some(chat) = self.store().chat(chat_id) else {
            return;
        };
        let Some(kind) = self.notice_kind(Place::ChatApps, &chat, event) else {
            return;
        };
        match event {
            ChatEvent::ApprovalRequested { approval, .. } => {
                for place in self.chat_places() {
                    self.show_request(&chat, approval, place.platform, &place.conversation);
                }
            }
            ChatEvent::GoalUpdated { goal: Some(goal) } => {
                let text = format!(
                    "{} met its goal:\n\n{}",
                    self.heading(&chat),
                    clip(&goal.objective, REPLY_LIMIT)
                );
                self.tell_finished(&chat, &text);
            }
            ChatEvent::TurnCompleted { .. } if kind == Kind::Usage => {
                let resets = self
                    .limit_reset(&chat)
                    .map(|at| format!(" It resets {}.", resets_in(at, now_ms())))
                    .unwrap_or_default();
                let text = format!("{} ran out of usage.{resets}", self.heading(&chat));
                let resume = Button {
                    label: "Continue when it resets".to_string(),
                    reply: format!("/resume {chat_id}"),
                };
                for place in self.chat_places() {
                    let sent = self.say_with(
                        place.platform,
                        &place.conversation,
                        &text,
                        std::slice::from_ref(&resume),
                    );
                    let Some(message_id) = sent else {
                        continue;
                    };
                    let notice = ResetNotice {
                        platform: place.platform,
                        conversation: place.conversation,
                        chat_id: chat_id.to_string(),
                        message_id,
                    };
                    if let Err(error) =
                        self.change_list(RESETS, |list: &mut Vec<ResetNotice>| list.push(notice))
                    {
                        log::warn!(error:% = error; "could not keep a usage notice: {{error}}");
                    }
                }
            }
            ChatEvent::TurnCompleted {
                message_id,
                status,
                error,
            } => {
                let Some(reply) = self.reply(chat_id, message_id, *status, error.clone()) else {
                    return;
                };
                let how = if kind == Kind::Failed {
                    "stopped"
                } else {
                    "finished"
                };
                let text = format!(
                    "{} {how}:\n\n{}",
                    self.heading(&chat),
                    clip(&reply, REPLY_LIMIT)
                );
                self.tell_finished(&chat, &text);
            }
            _ => {}
        }
    }

    fn tell_finished(&self, chat: &Chat, text: &str) {
        let mut buttons = vec![Button {
            label: "Mute this chat".to_string(),
            reply: format!("/mute {}", chat.id),
        }];
        if let Some(project) = &chat.project_id {
            buttons.push(Button {
                label: "Mute project".to_string(),
                reply: format!("/mute project {project}"),
            });
        }
        for place in self.chat_places() {
            self.say_with(place.platform, &place.conversation, text, &buttons);
        }
    }

    fn answer_notice(self: &Arc<Self>, platform: Platform, conversation: &str, text: &str) -> bool {
        let told: Vec<(String, String)> = HUB
            .lock()
            .notices
            .iter()
            .rev()
            .filter(|n| n.platform == platform && n.conversation == conversation)
            .map(|n| (n.chat_id.clone(), n.approval_id.clone()))
            .collect();
        for (chat_id, approval_id) in told {
            match self.approval_waiting(&chat_id, &approval_id) {
                Some(approval) => {
                    self.answer(platform, conversation, &chat_id, &approval, text);
                    return true;
                }
                // A request its turn left unanswered is no longer waiting.
                None => HUB.lock().notices.retain(|n| n.approval_id != approval_id),
            }
        }
        false
    }

    fn answer_by_id(
        self: &Arc<Self>,
        platform: Platform,
        conversation: &str,
        approval_id: &str,
        text: &str,
    ) {
        let chat_id = self.store().with_chats(|chats| {
            chats
                .iter()
                .find(|t| {
                    t.messages
                        .iter()
                        .any(|m| m.approvals.iter().any(|a| a.id == approval_id))
                })
                .map(|t| t.id.clone())
        });
        match chat_id.and_then(|id| Some((self.approval_waiting(&id, approval_id)?, id))) {
            Some((approval, id)) => self.answer(platform, conversation, &id, &approval, text),
            None => self.say(platform, conversation, "That request is no longer waiting."),
        }
    }

    fn approval_waiting(&self, chat_id: &str, approval_id: &str) -> Option<Approval> {
        if !self.turn_running(chat_id) {
            return None;
        }
        self.store()
            .chat(chat_id)?
            .messages
            .iter()
            .flat_map(|m| m.approvals.iter())
            .find(|a| a.id == approval_id && a.status == ApprovalStatus::Pending)
            .cloned()
    }

    fn close_notices(&self, approval_id: &str, allowed: bool, answered: bool) {
        let (closed, from) = {
            let mut state = HUB.lock();
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
            let text = outcome(
                allowed,
                answered,
                from.as_ref(),
                n.platform,
                &n.conversation,
            );
            self.say(n.platform, &n.conversation, &text);
        }
    }

    fn mute_notices(&self, rest: &str) -> String {
        let (project, id) = match rest.split_once(char::is_whitespace) {
            Some(("project", id)) => (true, id.trim()),
            _ => (false, rest),
        };
        let named = if project {
            self.store()
                .settings()
                .projects
                .into_iter()
                .find(|p| p.id == id)
                .map(|p| format!("chats in {}", p.name))
        } else {
            self.store()
                .chat(id)
                .map(|t| format!("\u{201c}{}\u{201d}", t.title))
        };
        let Some(named) = named else {
            return "Press a Mute button under a notice to stop hearing from that chat."
                .to_string();
        };
        let saved = if project {
            self.set_project_notices(id, false)
        } else {
            self.set_chat_notices(id, Some(Notices::Off))
        };
        match saved {
            Ok(()) => {
                format!("No more notices from {named}. Ask to hear from it again to undo this.")
            }
            Err(error) => format!("Patchwork could not mute that: {error}"),
        }
    }

    fn resume_at_reset(&self, chat_id: &str) -> String {
        let Some(chat) = self.store().chat(chat_id) else {
            return "Press Continue when it resets under a notice to pick the chat.".to_string();
        };
        match self.resume_at_limit_reset(chat_id) {
            Ok(at) => {
                self.emit(ChatEventEnvelope {
                    chat_id: chat_id.to_string(),
                    event: ChatEvent::OptionsChanged,
                });
                format!(
                    "\u{201c}{}\u{201d} continues {}.",
                    chat.title,
                    resets_in(at, now_ms())
                )
            }
            Err(error) => format!("Patchwork could not set that: {error}"),
        }
    }

    fn drop_reset_notices(&self, chat_id: &str) {
        // Every turn of every chat asks, so the list is only rewritten when it has some.
        let listed = self.read_list::<ResetNotice>(RESETS);
        if !listed.iter().any(|n| n.chat_id == chat_id) {
            return;
        }
        let mut dropped = Vec::new();
        let changed = self.change_list(RESETS, |list: &mut Vec<ResetNotice>| {
            let (gone, kept) = std::mem::take(list)
                .into_iter()
                .partition(|n| n.chat_id == chat_id);
            *list = kept;
            dropped = gone;
        });
        if let Err(error) = changed {
            log::warn!(error:% = error; "could not forget the usage notices: {{error}}");
        }
        let removals: Vec<_> = dropped
            .into_iter()
            .filter_map(|n| Some((HUB.connector(n.platform)?, n)))
            .collect();
        if removals.is_empty() {
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("gateway-drop-resets".into())
            .spawn(move || {
                for (connector, n) in removals {
                    if let Err(error) = connector.remove(&n.conversation, &n.message_id) {
                        log::warn!(platform = n.platform.id(), error:% = error; "could not delete a usage notice on {{platform}}: {{error}}");
                    }
                }
            });
        if let Err(error) = spawned {
            log::warn!(error:% = error; "could not start deleting usage notices: {{error}}");
        }
    }

    fn heading(&self, chat: &Chat) -> String {
        let project = chat.project_id.as_ref().and_then(|id| {
            self.store()
                .settings()
                .projects
                .into_iter()
                .find(|p| &p.id == id)
                .map(|p| p.name)
        });
        match project {
            Some(name) => format!("\u{201c}{}\u{201d} in {name}", chat.title),
            None => format!("\u{201c}{}\u{201d}", chat.title),
        }
    }
}
