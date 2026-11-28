//! Granular settings as the window drives them: opening the card on a chat or a project, starting
//! a chat that asks for them first, and what closing the card does to the chat box.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_chat::prelude::*;
use patchwork_client::AppState;
use patchwork_views_logos::AGENT_KINDS;
use std::sync::Arc;

use gpui::{Focusable, Window, prelude::*};
use patchwork_core::chats::NewChat;
use patchwork_desktop_dialogs::granular::{First, GranularDialog, GranularEvent, GranularScope};

use crate::window::Workspace;

impl Workspace {
    /// Opens the granular settings of a chat or a project. `first` is the message a new chat
    /// starts with, sent once the card is saved.
    pub fn open_granular_dialog(
        &mut self,
        scope: GranularScope,
        first: Option<First>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let composer = self.composer.read(cx);
        let models = AGENT_KINDS
            .into_iter()
            .filter_map(|kind| Some((kind, composer.models_of(kind)?.to_vec())))
            .collect();
        let Some(dialog) = GranularDialog::open(
            Arc::clone(&self.core),
            (*AppState::get(cx).settings).clone(),
            models,
            scope,
            first,
            window,
            cx,
        ) else {
            return;
        };
        cx.subscribe_in(&dialog, window, |this, _, e: &GranularEvent, window, cx| {
            let GranularEvent::Closed {
                scope,
                first,
                saved,
            } = e.clone();
            this.granular_closed(&scope, first, saved, window, cx);
        })
        .detach();
        self.dialogs_mut(cx, |d| d.granular = Some(dialog));
        cx.notify();
    }

    /// Starts the chat `new` and opens its granular settings, holding `first` until they are
    /// saved.
    pub fn start_granular_chat(
        &mut self,
        new: NewChat,
        role: Option<String>,
        first: First,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chat = match self.core.chats().create(new) {
            Ok(chat) => chat,
            Err(e) => {
                log::warn!(error:% = e; "could not start a chat: {{error}}");
                self.composer
                    .update(cx, |c, cx| c.put_back(&first.0, first.1, cx));
                return;
            }
        };
        let id = chat.id.clone();
        if role.is_some()
            && let Err(e) = self.core.set_chat_role(&id, role)
        {
            log::warn!(error:% = e; "could not set the chat's agent: {{error}}");
        }
        self.chat = Some(chat);
        let left = self.left_tab.take();
        self.keep_tab(
            left.as_deref(),
            AppState::get(cx).settings.tasks_in_new_tab,
            cx,
        );
        self.reload(cx);
        self.open_granular_dialog(GranularScope::Chat(id), Some(first), window, cx);
    }

    /// Drops the closed card. A held first message is sent when the settings were saved, and goes
    /// back to the chat box when they were not.
    fn granular_closed(
        &mut self,
        scope: &GranularScope,
        first: Option<First>,
        saved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialogs_mut(cx, |d| d.granular.take()).is_none() {
            return;
        }
        self.reload(cx);
        let open = self.chat.as_ref().map(|t| t.id.as_str());
        if matches!(scope, GranularScope::Chat(id) if open == Some(id.as_str())) {
            let editor = self.composer.read(cx).editor.clone();
            window.focus(&editor.read(cx).focus_handle(cx), cx);
        } else {
            window.focus(&self.focus, cx);
        }
        if let Some((text, attachments, queue)) = first {
            if saved {
                self.send_with(text, attachments, None, None, queue, cx);
            } else {
                self.composer
                    .update(cx, |c, cx| c.put_back(&text, attachments, cx));
            }
        }
        cx.notify();
    }
}
