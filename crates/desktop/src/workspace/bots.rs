//! Bots in the window: making one, opening its page in place of the chat, renaming and deleting.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{AnyElement, PromptLevel, Window, prelude::*};
use patchwork_core::bots::Bot;
use patchwork_core::threads::{Attachment, NewThread};
use crate::bots::{BotPage, Layout};
use super::state::{Place, Workspace};

impl Workspace {
    /// Reads the bots again, and closes the page of one that is gone or archived.
    pub fn reload_bots(&mut self, cx: &mut Context<Self>) {
        self.bots = self.core.bots();
        if let Some(id) = &self.bot_shown
            && !self
                .bots
                .iter()
                .any(|b| &b.id == id && b.archived_at.is_none())
        {
            self.bot_shown = None;
            self.bot_page = None;
        }
        cx.notify();
    }

    /// Reads the bots again after a picture was deleted or brought back, and shows the open
    /// bot's background as it now is.
    pub(super) fn bot_pictures_changed(&mut self, cx: &mut Context<Self>) {
        self.reload_bots(cx);
        let Some(page) = self.bot_page.clone() else {
            return;
        };
        let id = page.read(cx).bot.id.clone();
        if let Some(bot) = self.bots.iter().find(|b| b.id == id).cloned() {
            page.update(cx, |page, cx| page.show_saved(bot, cx));
        }
    }

    /// Makes a bot and opens its page.
    pub fn new_bot(&mut self, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        let task = cx.background_spawn(async move { core.create_bot() });
        cx.spawn(async move |this, cx| {
            let made = task.await;
            let _ = this.update(cx, |this, cx| match made {
                Ok(bot) => {
                    this.reload_bots(cx);
                    this.open_bot_page(&bot.id, cx);
                }
                Err(e) => {
                    this.error = Some(e.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Opens bot `id`'s chat, or its page before it has one.
    pub fn open_bot(&mut self, id: &str, cx: &mut Context<Self>) {
        match self.bot_chat(id) {
            Some(chat) => self.open_thread(&chat, cx),
            None => self.open_bot_page(id, cx),
        }
    }

    /// Opens bot `id`'s page.
    pub fn open_bot_page(&mut self, id: &str, cx: &mut Context<Self>) {
        self.go_to(&Place::Bot(id.to_string()), cx);
    }

    /// The id of bot `id`'s chat, while it is listed and not archived.
    pub fn bot_chat(&self, id: &str) -> Option<String> {
        let chat = self.bots.iter().find(|b| b.id == id)?.thread_id.as_ref()?;
        self.threads
            .iter()
            .any(|t| &t.id == chat && t.archived_at.is_none())
            .then(|| chat.clone())
    }

    /// Shows bot `id`'s page, and whether the bot exists.
    pub(super) fn show_bot(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        if !self
            .bots
            .iter()
            .any(|b| b.id == id && b.archived_at.is_none())
        {
            return false;
        }
        self.page = super::Page::Chats;
        self.peeking = false;
        // The composer under the page starts the bot's runs, so no chat is open behind it.
        self.thread = None;
        if self.bot_shown.as_deref() != Some(id)
            && let Some(bot) = self.bots.iter().find(|b| b.id == id)
        {
            self.composer.update(cx, |c, cx| c.pick_as(bot, cx));
        }
        self.bot_shown = Some(id.to_string());
        if self.view.is_none() {
            self.view = Some(super::View::Tasks);
        }
        cx.notify();
        true
    }

    /// The page of the bot shown, made the first time it is drawn.
    pub(super) fn bot_page_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let id = self.bot_shown.clone()?;
        let current = self
            .bot_page
            .as_ref()
            .is_some_and(|p| p.read(cx).bot.id == id && p.read(cx).layout == Layout::Page);
        if !current {
            let bot = self.bots.iter().find(|b| b.id == id).cloned()?;
            let (workspace, core) = (cx.entity().downgrade(), Arc::clone(&self.core));
            self.bot_page =
                Some(cx.new(|cx| BotPage::new(workspace, core, bot, Layout::Page, window, cx)));
        }
        self.bot_page
            .clone()
            .map(gpui::IntoElement::into_any_element)
    }

    /// The bot whose chat is open, when one of its chats is open in place of its page.
    pub fn chat_bot(&self) -> Option<&Bot> {
        if self.bot_shown.is_some() {
            return None;
        }
        let id = self.thread.as_ref()?.bot_id.as_ref()?;
        self.bots
            .iter()
            .find(|b| &b.id == id && b.archived_at.is_none())
    }

    /// Shows or hides the bot's details beside its chat.
    pub fn toggle_bot_details(&mut self, cx: &mut Context<Self>) {
        self.bot_details = !self.bot_details;
        cx.notify();
    }

    /// The details of the bot whose chat is open, beside it, while they are shown.
    pub(super) fn bot_details_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.bot_details {
            return None;
        }
        let bot = self.chat_bot()?.clone();
        let current = self.bot_page.as_ref().is_some_and(|p| {
            p.read(cx).bot.id == bot.id && p.read(cx).layout == Layout::BesideChat
        });
        if !current {
            let (workspace, core) = (cx.entity().downgrade(), Arc::clone(&self.core));
            self.bot_page = Some(
                cx.new(|cx| BotPage::new(workspace, core, bot, Layout::BesideChat, window, cx)),
            );
        }
        self.bot_page
            .clone()
            .map(gpui::IntoElement::into_any_element)
    }

    /// Opens bot `id`'s chat with its details beside it, or its page before it has a chat.
    pub fn open_bot_details(&mut self, id: &str, cx: &mut Context<Self>) {
        match self.bot_chat(id) {
            Some(chat) => {
                self.bot_details = true;
                self.open_thread(&chat, cx);
            }
            None => self.open_bot_page(id, cx),
        }
    }

    /// Renames bot `id` off the UI thread, and shows the new name in the list and on its page.
    pub fn rename_bot(&mut self, id: String, name: String, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        let task = cx.background_spawn(async move { core.rename_bot(&id, &name) });
        cx.spawn(async move |this, cx| {
            let renamed = task.await;
            let _ = this.update(cx, |this, cx| {
                match renamed {
                    Ok(bot) => {
                        if let Some(page) = &this.bot_page {
                            page.update(cx, |page, cx| page.show_saved(bot, cx));
                        }
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                this.reload_bots(cx);
            });
        })
        .detach();
    }

    /// Moves bot `id` out of the sidebar when `archived`, or back into it, off the UI thread.
    pub fn set_bot_archived(&mut self, id: &str, archived: bool, cx: &mut Context<Self>) {
        let (core, id) = (Arc::clone(&self.core), id.to_string());
        let task = cx.background_spawn(async move { core.set_bot_archived(&id, archived) });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = done {
                    log::warn!(error:% = e; "could not archive or restore the bot: {{error}}");
                    this.error = Some(e.to_string());
                }
                this.reload_bots(cx);
            });
        })
        .detach();
    }

    /// Sends `text` and `attachments` to the chat of the bot shown, starting it with the
    /// composer's picks in `new` in the project picked on its page, and opens it. False when no bot is shown or a
    /// run is already starting, so the composer keeps the message.
    pub fn send_to_bot(
        &mut self,
        text: String,
        attachments: Vec<Attachment>,
        mut new: NewThread,
        cx: &mut Context<Self>,
    ) -> bool {
        let (Some(id), Some(page)) = (self.bot_shown.clone(), self.bot_page.clone()) else {
            return false;
        };
        if page.read(cx).starting {
            return false;
        }
        new.project_id = page.read(cx).run_place();
        page.update(cx, |p, cx| p.set_starting(true, cx));
        let (core, kept) = (Arc::clone(&self.core), (text.clone(), attachments.clone()));
        let task =
            cx.background_spawn(async move { core.run_bot_with(&id, new, &text, attachments) });
        cx.spawn(async move |this, cx| {
            let started = task.await;
            let _ = this.update(cx, |this, cx| {
                page.update(cx, |p, cx| p.set_starting(false, cx));
                match started {
                    Ok(thread) => {
                        this.reload_bots(cx);
                        // The run saved the picks it started with on the bot.
                        let saved = thread
                            .bot_id
                            .as_ref()
                            .and_then(|id| this.bots.iter().find(|b| &b.id == id))
                            .cloned();
                        page.update(cx, |p, cx| {
                            if let Some(bot) = saved {
                                p.show_saved(bot, cx);
                            }
                            p.load_usage(cx);
                        });
                        this.reload(cx);
                        this.open_thread(&thread.id, cx);
                    }
                    Err(e) => {
                        log::warn!(error:% = e; "could not start the bot's run: {{error}}");
                        this.error = Some(e.to_string());
                        let (text, attachments) = kept;
                        this.composer
                            .update(cx, |c, cx| c.put_back(&text, attachments, cx));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        true
    }

    /// Opens Settings on Scheduled with a new routine that runs as `bot`.
    pub fn add_bot_routine(&mut self, bot: &Bot, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings(Some("scheduled"), window, cx);
        if let Some(dialog) = &self.settings_dialog {
            dialog.update(cx, |d, cx| d.add_routine(bot, cx));
        }
    }

    /// Asks, then deletes bot `id`. Its conversations stay, as chats.
    pub fn delete_bot(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .bots
            .iter()
            .find(|b| b.id == id)
            .map_or_else(|| "this bot".to_string(), |b| b.name.clone());
        let answer = window.prompt(
            PromptLevel::Warning,
            "Delete bot",
            Some(&format!("Delete {name}? Its conversations stay as chats.")),
            &["Delete", "Cancel"],
            cx,
        );
        let (core, id) = (Arc::clone(&self.core), id.to_string());
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let removed = cx
                .background_spawn(async move { core.remove_bot(&id) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = removed {
                    log::warn!(error:% = e; "could not delete the bot: {{error}}");
                    this.error = Some(e.to_string());
                }
                // Drops its page, and lists its conversations with the chats.
                this.reload_bots(cx);
                this.reload(cx);
            });
        })
        .detach();
    }
}
