//! A chat's or an agent's own background as the window drives it: opening the picker's card,
//! saving each pick, and closing it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{Window, prelude::*};

use patchwork_backend::backgrounds::ChatBackground;

use crate::workspace::Workspace;
use patchwork_desktop_settings::background_picker::{BackgroundPicker, PickerEvent};
use patchwork_desktop_settings::background::{BackgroundDialog, Closed};
use patchwork_ui_markdown::display_title;

/// Whose background the dialog picks.
#[derive(Clone)]
pub(crate) enum Owner {
    Chat(String),
    Agent(String),
}

impl Workspace {
    /// Opens the background of the chat `id`; a subagent's chat has none of its own.
    pub fn open_background_dialog(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(chat) = self.core.chat(id) else {
            return;
        };
        if chat.parent.is_some() {
            return;
        }
        let name = display_title(&chat.title);
        let owner = Owner::Chat(chat.id);
        self.open_picker(
            owner,
            name,
            "Background of this chat",
            chat.background,
            window,
            cx,
        );
    }

    /// Opens the background of agent `id`, which its page and its runs show.
    pub fn open_agent_background_dialog(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.core.agent(id) else {
            return;
        };
        let owner = Owner::Agent(agent.id);
        self.open_picker(
            owner,
            agent.name,
            "Background of this agent",
            agent.background,
            window,
            cx,
        );
    }

    /// The background `owner` has of its own, read again.
    fn own_background(&self, owner: &Owner) -> Option<ChatBackground> {
        match owner {
            Owner::Chat(id) => self.core.chat(id).ok().and_then(|t| t.background),
            Owner::Agent(id) => self.core.agent(id).and_then(|b| b.background),
        }
    }

    /// Saves `background` as `owner`'s own, and shows it on the agent's page.
    fn save_background(
        &mut self,
        owner: &Owner,
        background: Option<ChatBackground>,
        cx: &mut Context<Self>,
    ) -> Result<(), patchwork_backend::Error> {
        match owner {
            Owner::Chat(id) => self.core.set_chat_background(id, background),
            Owner::Agent(id) => {
                let agent = self.core.set_agent_background(id, background)?;
                if let Some(page) = &self.agent_page {
                    page.update(cx, |page, cx| page.show_saved(agent, cx));
                }
                self.reload_agents(cx);
                Ok(())
            }
        }
    }

    fn open_picker(
        &mut self,
        owner: Owner,
        name: String,
        label: &'static str,
        value: Option<ChatBackground>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (core, shared) = (
            Arc::clone(&self.core),
            self.settings.chat_background.clone(),
        );
        let picker = cx.new(|cx| BackgroundPicker::new(core, label, value, None, Some(shared), cx));
        cx.subscribe(&picker, |this, picker, e: &PickerEvent, cx| {
            let Some(owner) = this
                .dialogs
                .read(cx)
                .background
                .as_ref()
                .map(|(o, _)| o.clone())
            else {
                return;
            };
            match e {
                PickerEvent::Picked(background) => {
                    if let Err(e) = this.save_background(&owner, background.clone(), cx) {
                        let before = this.own_background(&owner);
                        picker.update(cx, |p, cx| p.failed(before, e.to_string(), cx));
                    }
                }
                // The pictures moved the backgrounds that showed them: this one, others', and
                // the one all chats share.
                PickerEvent::PicturesChanged => {
                    this.backgrounds.reload();
                    this.settings = this.core.settings();
                    this.agent_pictures_changed(cx);
                    let value = this.own_background(&owner);
                    let shared = this.settings.chat_background.clone();
                    picker.update(cx, |p, cx| {
                        p.set_value(value, Some(shared));
                        cx.notify();
                    });
                }
            }
            this.reload(cx);
        })
        .detach();
        let dialog = cx.new(|cx| BackgroundDialog::new(name, picker, window, cx));
        cx.subscribe_in(&dialog, window, |this, _, _: &Closed, window, cx| {
            this.close_background_dialog(window, cx);
        })
        .detach();
        self.dialogs_mut(cx, |d| d.background = Some((owner, dialog)));
        cx.notify();
    }

    fn close_background_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialogs_mut(cx, |d| d.background.take()).is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }
}
