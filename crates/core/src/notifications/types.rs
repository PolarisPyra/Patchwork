//! What notifications people asked for, and the notice the desktop shows.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Which events notify, where, and for which chats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationSettings {
    /// What the desktop shows while Patchwork is not the focused window.
    pub desktop: Events,
    /// What the paired people on the chat apps are sent.
    pub chat_apps: Events,
    /// When the chat apps are sent anything.
    pub chat_apps_when: ChatAppsWhen,
    /// How long Patchwork must be out of focus, in seconds, before you count as away.
    pub away_after_secs: u32,
    /// The chats that have their own choice, by thread id. It wins over the events above and
    /// over a muted project.
    pub threads: BTreeMap<String, Notices>,
    /// The projects whose chats without their own choice notify nowhere.
    pub muted_projects: Vec<String>,
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
            threads: BTreeMap::new(),
            muted_projects: Vec::new(),
        }
    }
}

impl NotificationSettings {
    /// The chat's own choice, or nothing when its project is muted; None when it follows the
    /// events chosen for each place.
    #[must_use]
    pub fn chat(&self, thread_id: &str, project_id: Option<&str>) -> Option<Notices> {
        if let Some(own) = self.threads.get(thread_id) {
            return Some(*own);
        }
        project_id
            .is_some_and(|p| self.muted_projects.iter().any(|m| m == p))
            .then_some(Notices::Off)
    }

    /// Whether `kind` in the chat `thread_id` of `project_id` notifies at `place`.
    #[must_use]
    pub fn wants(
        &self,
        place: Place,
        kind: Kind,
        thread_id: &str,
        project_id: Option<&str>,
    ) -> bool {
        match self.chat(thread_id, project_id) {
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
    /// id for a question, else the thread's id and the event.
    pub tag: String,
    pub title: String,
    pub body: String,
}
