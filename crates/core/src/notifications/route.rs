//! Which events notify, and the desktop's notices: what each says, and when it is sent.

// Rust guideline compliant 2026-10-09

use super::types::{DesktopNotice, Kind, Notices, Place};
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::system::now_ms;
use crate::chats::{ApprovalStatus, GoalStatus, MessageStatus, Chat};
use crate::{Core, Error};

/// The most characters of a reply or request a desktop notice shows.
const BODY_LIMIT: usize = 200;

impl Core {
    /// The kind of notification `event` of `chat` makes at `place`, when it makes one there:
    /// the chat is one a person started, and the settings ask for that kind there.
    pub(crate) fn notice_kind(
        &self,
        place: Place,
        chat: &Chat,
        event: &AgentEvent,
    ) -> Option<Kind> {
        if chat.parent.is_some() {
            return None;
        }
        let kind = match event {
            AgentEvent::ApprovalRequested { approval, .. }
                if !approval.auto && approval.status == ApprovalStatus::Pending =>
            {
                Kind::Asks
            }
            AgentEvent::TurnCompleted { status, .. } => match status {
                // A goal still being worked toward runs another turn; it notifies once met.
                MessageStatus::Complete
                    if chat
                        .goal
                        .as_ref()
                        .is_none_or(|g| g.status != GoalStatus::Active) =>
                {
                    Kind::Finished
                }
                MessageStatus::Error if self.limit_reset(chat).is_some() => Kind::Usage,
                MessageStatus::Error => Kind::Failed,
                _ => return None,
            },
            AgentEvent::GoalUpdated { goal: Some(goal) } if goal.status == GoalStatus::Complete => {
                if !self.presence.first_goal(place, &chat.id, goal.started_at) {
                    return None;
                }
                Kind::Goal
            }
            _ => return None,
        };
        self.store
            .settings()
            .notifications
            .wants(place, kind, &chat.id, chat.project_id.as_deref())
            .then_some(kind)
    }

    /// Sends the desktop a notice of `event`, while Patchwork's window is out of focus and the
    /// settings ask for one. A chat a chat app started tells only the chat app.
    pub(crate) fn notify_desktop(&self, chat_id: &str, event: &AgentEvent) {
        let relevant = matches!(
            event,
            AgentEvent::ApprovalRequested { .. }
                | AgentEvent::TurnCompleted { .. }
                | AgentEvent::GoalUpdated { goal: Some(_) }
        );
        if !relevant || self.window_focused() || self.in_background(chat_id) {
            return;
        }
        let Some(chat) = self.store.chat(chat_id) else {
            return;
        };
        let Some(kind) = self.notice_kind(Place::Desktop, &chat, event) else {
            return;
        };
        let title = &chat.title;
        let (tag, title, body) = match (kind, event) {
            (Kind::Asks, AgentEvent::ApprovalRequested { approval, .. }) => (
                approval.id.clone(),
                format!("{title} asks"),
                approval.title.clone(),
            ),
            (Kind::Goal, AgentEvent::GoalUpdated { goal: Some(goal) }) => (
                format!("{chat_id}/goal"),
                format!("{title} met its goal"),
                goal.objective.clone(),
            ),
            (Kind::Usage, AgentEvent::TurnCompleted { .. }) => {
                let resets = self
                    .limit_reset(&chat)
                    .map(|at| format!("Usage resets {}.", resets_in(at, now_ms())))
                    .unwrap_or_default();
                (
                    format!("{chat_id}/turn"),
                    format!("{title} ran out of usage"),
                    resets,
                )
            }
            (
                _,
                AgentEvent::TurnCompleted {
                    message_id, error, ..
                },
            ) => {
                let message = chat.messages.iter().find(|m| &m.id == message_id);
                let body = if kind == Kind::Failed {
                    error
                        .clone()
                        .or_else(|| message.and_then(|m| m.error.clone()))
                        .unwrap_or_default()
                } else {
                    message
                        .map(|m| m.text.trim().to_string())
                        .unwrap_or_default()
                };
                let how = if kind == Kind::Failed {
                    "failed"
                } else {
                    "finished"
                };
                (format!("{chat_id}/turn"), format!("{title} {how}"), body)
            }
            _ => return,
        };
        (self.sink)(AgentEventEnvelope {
            chat_id: chat_id.to_string(),
            event: AgentEvent::Notice {
                notice: DesktopNotice {
                    tag,
                    title,
                    body: shorten(&body),
                },
            },
        });
    }

    /// Gives the chat `chat_id` its own choice of notifications, or, with none, the one its
    /// project and Settings make.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be saved.
    pub fn set_chat_notices(&self, chat_id: &str, notices: Option<Notices>) -> Result<(), Error> {
        let id = chat_id.to_string();
        self.update_settings(move |s| match notices {
            Some(notices) => _ = s.notifications.chats.insert(id, notices),
            None => _ = s.notifications.chats.remove(&id),
        })
    }

    /// Lets the chats of `project_id` without their own choice notify, or mutes them.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be saved.
    pub fn set_project_notices(&self, project_id: &str, on: bool) -> Result<(), Error> {
        let id = project_id.to_string();
        self.update_settings(move |s| {
            let muted = &mut s.notifications.muted_projects;
            muted.retain(|m| *m != id);
            if !on {
                muted.push(id);
            }
        })
    }

    /// Turns every muted chat and project back on, and says how many there were.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be saved.
    pub fn unmute_all(&self) -> Result<usize, Error> {
        self.update_settings(|s| {
            let n = &mut s.notifications;
            let before = n.chats.len() + n.muted_projects.len();
            n.chats.retain(|_, own| *own != Notices::Off);
            n.muted_projects.clear();
            before - n.chats.len()
        })
    }
}

/// How long until `at`, from `now`, both in Unix milliseconds: "in 3h 12m", "in 2d 4h", "in 5m".
pub(crate) fn resets_in(at: u64, now: u64) -> String {
    let mins = at.saturating_sub(now).div_ceil(60_000);
    let (d, h, m) = (mins / 1440, mins % 1440 / 60, mins % 60);
    if d > 0 {
        format!("in {d}d {h}h")
    } else if h > 0 {
        format!("in {h}h {m}m")
    } else {
        format!("in {m}m")
    }
}

/// `text` on one line, cut to `BODY_LIMIT` characters.
fn shorten(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= BODY_LIMIT {
        return line;
    }
    let cut: String = line.chars().take(BODY_LIMIT - 1).collect();
    format!("{}…", cut.trim_end())
}
