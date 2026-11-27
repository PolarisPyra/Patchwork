//! Picking the background of the Orchestrator, of chats or of Quilts, which picks it for all
//! three while backgrounds are kept in sync.

// Rust guideline compliant 2026-11-23

use patchwork_themes::{BackdropChoice, Scheme};
use crate::backgrounds::ChatBackground;
use crate::settings::Settings;

impl From<ChatBackground> for BackdropChoice {
    fn from(background: ChatBackground) -> Self {
        match background {
            ChatBackground::None => Self::None,
            ChatBackground::Butterfly => Self::Butterfly,
            ChatBackground::Preset { id } => Self::Preset { id },
            ChatBackground::Image { file } => Self::Image { file },
            ChatBackground::Scene { id } => Self::Scene { id },
        }
    }
}

impl From<BackdropChoice> for ChatBackground {
    fn from(choice: BackdropChoice) -> Self {
        match choice {
            BackdropChoice::None => Self::None,
            BackdropChoice::Butterfly => Self::Butterfly,
            BackdropChoice::Preset { id } => Self::Preset { id },
            BackdropChoice::Image { file } => Self::Image { file },
            BackdropChoice::Scene { id } => Self::Scene { id },
        }
    }
}

impl Settings {
    /// Puts `background` behind every chat, and behind every page while they are in sync.
    pub fn pick_chat_background(&mut self, background: ChatBackground) {
        if self.sync_backgrounds {
            self.share_background(background);
        } else {
            self.chat_background = background;
        }
    }

    /// Puts `background` behind the Quilts page, and behind every page while they are in sync.
    pub fn pick_quilts_background(&mut self, background: ChatBackground) {
        if self.sync_backgrounds {
            self.share_background(background);
        } else {
            self.quilts_background = Some(background);
        }
    }

    /// Puts `choice` behind the Orchestrator in windows of `scheme`, and behind every page in
    /// every window while they are in sync.
    pub fn pick_backdrop(&mut self, scheme: Option<Scheme>, choice: BackdropChoice) {
        if self.sync_backgrounds {
            self.share_background(choice.into());
        } else {
            *self.backdrop_mut(scheme) = choice;
        }
    }

    /// Keeps the backgrounds in sync or lets each page have its own. Turning it on gives every
    /// page the chats' background.
    pub fn set_sync_backgrounds(&mut self, on: bool) {
        self.sync_backgrounds = on;
        if on {
            self.share_background(self.chat_background.clone());
        }
    }

    /// Shows `background` behind chats, Quilts and the Orchestrator in every window.
    fn share_background(&mut self, background: ChatBackground) {
        self.quilts_background = Some(background.clone());
        self.orchestrator_backdrop = background.clone().into();
        self.system_light_backdrop = None;
        self.system_dark_backdrop = None;
        self.chat_background = background;
    }
}
