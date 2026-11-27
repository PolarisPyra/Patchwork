//! Which events of a chat notify, and where: the desktop, the chat apps, or both.
//!
//! The chat apps' side of sending is in
//! `crate::gateway`; the desktop shows the [`DesktopNotice`]s this sends it.

// Rust guideline compliant 2026-10-09

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::system::{Locked, now_ms};
use crate::{Core, Error};
use crate::events::{ChatEvent, ToWindow, WindowCommand};
use crate::chats::{ApprovalStatus, GoalStatus, MessageStatus, Chat};

/// Which events notify, where, and for which chats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationSettings {
    /// What the desktop shows while Patchwork is not the focused window.
    pub desktop: Events,
    /// What the pairings on the chat apps are sent.
    pub chat_apps: Events,
    /// When the chat apps are sent anything.
    pub chat_apps_when: ChatAppsWhen,
    /// How long Patchwork must be out of focus, in seconds, before you count as away.
    pub away_after_secs: u32,
    /// The chats that have their own choice, by chat id. It wins over the events above and
    /// over a muted project.
    pub chats: BTreeMap<String, Notices>,
    /// The projects whose chats without their own choice notify nowhere.
    pub muted_projects: Vec<String>,
    /// Whether the desktop says when a connected agent CLI has a newer release.
    pub cli_updates: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            desktop: Events {
                asks: true,
                finished: true,
                goal: true,
                failed: true,
                usage: true,
            },
            chat_apps: Events::default(),
            chat_apps_when: ChatAppsWhen::Away,
            away_after_secs: 300,
            chats: BTreeMap::new(),
            muted_projects: Vec::new(),
            cli_updates: true,
        }
    }
}

impl NotificationSettings {
    /// The chat's own choice, or nothing when its project is muted; None when it follows the
    /// events chosen for each place.
    #[must_use]
    pub fn chat(&self, chat_id: &str, project_id: Option<&str>) -> Option<Notices> {
        if let Some(own) = self.chats.get(chat_id) {
            return Some(*own);
        }
        project_id
            .is_some_and(|p| self.muted_projects.iter().any(|m| m == p))
            .then_some(Notices::Off)
    }

    /// Whether `kind` in the chat `chat_id` of `project_id` notifies at `place`.
    #[must_use]
    pub fn wants(&self, place: Place, kind: Kind, chat_id: &str, project_id: Option<&str>) -> bool {
        match self.chat(chat_id, project_id) {
            Some(own) => own.allows(kind),
            None => match place {
                Place::Desktop => self.desktop.allows(kind),
                Place::ChatApps => self.chat_apps.allows(kind),
            },
        }
    }
}

/// The events that notify at one place.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate setting the user turns on or off"
)]
pub struct Events {
    /// A chat asks for permission or asks a question.
    pub asks: bool,
    /// A chat finishes replying, outside a goal it is still working toward.
    pub finished: bool,
    /// A chat meets its goal.
    pub goal: bool,
    /// A chat's reply ends in an error.
    pub failed: bool,
    /// A chat's reply stops because its agent's usage limit ran out.
    pub usage: bool,
}

impl Events {
    /// Whether `kind` is one of them.
    #[must_use]
    pub fn allows(self, kind: Kind) -> bool {
        match kind {
            Kind::Asks => self.asks,
            Kind::Finished => self.finished,
            Kind::Goal => self.goal,
            Kind::Failed => self.failed,
            Kind::Usage => self.usage,
        }
    }

    /// The events with `kind` turned on or off.
    #[must_use]
    pub fn with(mut self, kind: Kind, on: bool) -> Self {
        match kind {
            Kind::Asks => self.asks = on,
            Kind::Finished => self.finished = on,
            Kind::Goal => self.goal = on,
            Kind::Failed => self.failed = on,
            Kind::Usage => self.usage = on,
        }
        self
    }
}

/// An event worth a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Asks,
    Finished,
    Goal,
    Failed,
    /// The agent's usage limit ran out; the turn can resume once it resets.
    Usage,
}

/// Where a notification goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Place {
    Desktop,
    ChatApps,
}

/// When the chat apps are sent notifications.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatAppsWhen {
    /// Once Patchwork has been out of focus for `away_after_secs`.
    #[default]
    Away,
    /// Whenever the event notifies.
    Always,
}

/// A chat's own choice of what notifies, at every place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Notices {
    /// Every event.
    All,
    /// Only when it asks something.
    Asks,
    /// Only when it finishes, meets its goal, fails or runs out of usage.
    Finished,
    /// Nothing.
    Off,
}

impl Notices {
    /// Whether `kind` notifies.
    #[must_use]
    pub fn allows(self, kind: Kind) -> bool {
        match self {
            Self::All => true,
            Self::Asks => kind == Kind::Asks,
            Self::Finished => kind != Kind::Asks,
            Self::Off => false,
        }
    }
}

/// A notification for the desktop to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopNotice {
    /// Names the notice, so a later one replaces it and an answer takes it down: the request's
    /// id for a question, else the chat's id and the event.
    pub tag: String,
    pub title: String,
    pub body: String,
}

/// The window's focus, and the goals already told as met at each place.
#[derive(Debug, Default)]
pub(crate) struct Presence {
    focused: AtomicBool,
    /// When the window last lost focus, in Unix milliseconds; 0 until the desktop reports.
    since: AtomicU64,
    /// When a key or the mouse was last used in the window, in Unix milliseconds; 0 until the
    /// desktop reports.
    active: AtomicU64,
    /// The start of the goal each chat was last told as met, by place and chat id, so a goal
    /// reported again as met is told once.
    goals: Mutex<HashMap<(Place, String), u64>>,
}

impl Presence {
    /// Records that `started_at`'s goal of `chat_id` was met, and says whether it is the first
    /// time at `place`.
    pub(crate) fn first_goal(&self, place: Place, chat_id: &str, started_at: u64) -> bool {
        let mut goals = self.goals.locked();
        goals.insert((place, chat_id.to_string()), started_at) != Some(started_at)
    }
}

impl Core {
    /// Records whether Patchwork's window has focus. The desktop calls it each time that
    /// changes; while it has focus, nothing notifies.
    pub fn set_window_focused(&self, focused: bool) {
        let presence = &self.presence;
        if presence.focused.swap(focused, Ordering::SeqCst) != focused && !focused {
            presence.since.store(now_ms(), Ordering::SeqCst);
        }
    }

    /// Records that a key or the mouse was used in Patchwork's window just now.
    pub fn note_activity(&self) {
        self.presence.active.store(now_ms(), Ordering::Relaxed);
    }

    /// How long ago the window was last used, in milliseconds: 0 until the desktop reports.
    pub(crate) fn idle_ms(&self) -> u64 {
        match self.presence.active.load(Ordering::Relaxed) {
            0 => 0,
            at => now_ms().saturating_sub(at),
        }
    }

    /// Whether Patchwork's window has focus.
    pub(crate) fn window_focused(&self) -> bool {
        self.presence.focused.load(Ordering::SeqCst)
    }

    /// Whether the window has been out of focus for as long as the settings call away.
    pub(crate) fn away(&self) -> bool {
        if self.window_focused() {
            return false;
        }
        let after = u64::from(self.store.settings().notifications.away_after_secs) * 1000;
        now_ms().saturating_sub(self.presence.since.load(Ordering::SeqCst)) >= after
    }
}

/// The most characters of a reply or request a desktop notice shows.
const BODY_LIMIT: usize = 200;

impl Core {
    /// The kind of notification `event` of `chat` makes at `place`, when it makes one there:
    /// the chat is one a person started, and the settings ask for that kind there.
    pub(crate) fn notice_kind(&self, place: Place, chat: &Chat, event: &ChatEvent) -> Option<Kind> {
        if chat.parent.is_some() {
            return None;
        }
        let kind = match event {
            ChatEvent::ApprovalRequested { approval, .. }
                if !approval.auto && approval.status == ApprovalStatus::Pending =>
            {
                Kind::Asks
            }
            ChatEvent::TurnCompleted { status, .. } => match status {
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
            ChatEvent::GoalUpdated { goal: Some(goal) } if goal.status == GoalStatus::Complete => {
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
    pub(crate) fn notify_desktop(&self, chat_id: &str, event: &ChatEvent) {
        if !event.may_notify() || self.window_focused() || self.in_background(chat_id) {
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
            (Kind::Asks, ChatEvent::ApprovalRequested { approval, .. }) => (
                approval.id.clone(),
                format!("{title} asks"),
                approval.title.clone(),
            ),
            (Kind::Goal, ChatEvent::GoalUpdated { goal: Some(goal) }) => (
                format!("{chat_id}/goal"),
                format!("{title} met its goal"),
                goal.objective.clone(),
            ),
            (Kind::Usage, ChatEvent::TurnCompleted { .. }) => {
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
                ChatEvent::TurnCompleted {
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
        // Sent to the window alone: a notice is not a chat event, and the log leaves it out.
        (self.sink)(ToWindow::Command(WindowCommand::Notify {
            chat_id: chat_id.to_string(),
            notice: DesktopNotice {
                tag,
                title,
                body: shorten(&body),
            },
        }));
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
