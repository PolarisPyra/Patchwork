//! Agents in the window: making one, opening its page in place of the chat, renaming and deleting.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{AnyElement, PromptLevel, Window, prelude::*};
use patchwork_backend::agents::Agent;
use patchwork_backend::chats::{Attachment, NewChat};
use crate::agents::{AgentPage, Layout};
use crate::workspace::{Place, Workspace};

impl Workspace {
    /// Reads the agents again, and closes the page of one that is gone or archived.
    pub fn reload_agents(&mut self, cx: &mut Context<Self>) {
        self.agents = self.core.agents();
        if let Some(id) = &self.agent_shown
            && !self
                .agents
                .iter()
                .any(|b| &b.id == id && b.archived_at.is_none())
        {
            self.agent_shown = None;
            self.agent_page = None;
        }
        cx.notify();
    }

    /// Reads the agents again after a picture was deleted or brought back, and shows the open
    /// agent's background as it now is.
    pub(crate) fn agent_pictures_changed(&mut self, cx: &mut Context<Self>) {
        self.reload_agents(cx);
        let Some(page) = self.agent_page.clone() else {
            return;
        };
        let id = page.read(cx).agent.id.clone();
        if let Some(agent) = self.agents.iter().find(|b| b.id == id).cloned() {
            page.update(cx, |page, cx| page.show_saved(agent, cx));
        }
    }

    /// Makes an agent and opens its page.
    pub fn new_agent(&mut self, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        patchwork_ui_theme::off_thread(
            cx,
            move || core.create_agent(),
            move |this, made, cx| match made {
                Ok(agent) => {
                    this.reload_agents(cx);
                    this.open_agent_page(&agent.id, cx);
                }
                Err(e) => {
                    this.error = Some(e.to_string());
                    cx.notify();
                }
            },
        );
    }

    /// Opens agent `id`'s chat, or its page before it has one.
    pub fn open_agent(&mut self, id: &str, cx: &mut Context<Self>) {
        match self.agent_chat(id) {
            Some(chat) => self.open_chat(&chat, cx),
            None => self.open_agent_page(id, cx),
        }
    }

    /// Whether the open chat is a chat-app agent's, which shows each Patchwork tool as a note.
    pub fn shows_notes(&self) -> bool {
        self.chat_agent().is_some_and(|b| b.chat_app.is_some())
    }

    /// Opens agent `id`'s page.
    pub fn open_agent_page(&mut self, id: &str, cx: &mut Context<Self>) {
        self.go_to(&Place::Agent(id.to_string()), cx);
    }

    /// The id of agent `id`'s chat, while it is listed and not archived.
    pub fn agent_chat(&self, id: &str) -> Option<String> {
        let chat = self.agents.iter().find(|b| b.id == id)?.chat_id.as_ref()?;
        self.chats
            .iter()
            .any(|t| &t.id == chat && t.archived_at.is_none())
            .then(|| chat.clone())
    }

    /// Shows agent `id`'s page, and whether the agent exists.
    pub(crate) fn show_agent(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        if !self
            .agents
            .iter()
            .any(|b| b.id == id && b.archived_at.is_none())
        {
            return false;
        }
        self.page = crate::workspace::Page::Chats;
        self.peeking = false;
        // The composer under the page starts the agent's runs, so no chat is open behind it.
        self.close_chat(cx);
        if self.agent_shown.as_deref() != Some(id)
            && let Some(agent) = self.agents.iter().find(|b| b.id == id)
        {
            self.composer.update(cx, |c, cx| c.pick_as(agent, cx));
        }
        self.agent_shown = Some(id.to_string());
        if self.view.is_none() {
            self.view = Some(crate::workspace::View::Tasks);
        }
        cx.notify();
        true
    }

    /// The page of the agent shown, made the first time it is drawn.
    pub(crate) fn agent_page_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let id = self.agent_shown.clone()?;
        let current = self
            .agent_page
            .as_ref()
            .is_some_and(|p| p.read(cx).agent.id == id && p.read(cx).layout == Layout::Page);
        if !current {
            let agent = self.agents.iter().find(|b| b.id == id).cloned()?;
            let (workspace, core) = (cx.entity().downgrade(), Arc::clone(&self.core));
            self.agent_page =
                Some(cx.new(|cx| AgentPage::new(workspace, core, agent, Layout::Page, window, cx)));
        }
        self.agent_page
            .clone()
            .map(gpui::IntoElement::into_any_element)
    }

    /// The agent whose chat is open, when one of its chats is open in place of its page.
    pub fn chat_agent(&self) -> Option<&Agent> {
        if self.agent_shown.is_some() {
            return None;
        }
        let id = self.chat.as_ref()?.agent_id.as_ref()?;
        self.agents
            .iter()
            .find(|b| &b.id == id && b.archived_at.is_none())
    }

    /// Shows or hides the agent's details beside its chat.
    pub fn toggle_agent_details(&mut self, cx: &mut Context<Self>) {
        self.agent_details = !self.agent_details;
        cx.notify();
    }

    /// The details of the agent whose chat is open, beside it, while they are shown.
    pub(crate) fn agent_details_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.agent_details {
            return None;
        }
        let agent = self.chat_agent()?.clone();
        let current = self.agent_page.as_ref().is_some_and(|p| {
            p.read(cx).agent.id == agent.id && p.read(cx).layout == Layout::BesideChat
        });
        if !current {
            let (workspace, core) = (cx.entity().downgrade(), Arc::clone(&self.core));
            self.agent_page =
                Some(cx.new(|cx| {
                    AgentPage::new(workspace, core, agent, Layout::BesideChat, window, cx)
                }));
        }
        self.agent_page
            .clone()
            .map(gpui::IntoElement::into_any_element)
    }

    /// Opens agent `id`'s chat with its details beside it, or its page before it has a chat.
    pub fn open_agent_details(&mut self, id: &str, cx: &mut Context<Self>) {
        match self.agent_chat(id) {
            Some(chat) => {
                self.agent_details = true;
                self.open_chat(&chat, cx);
            }
            None => self.open_agent_page(id, cx),
        }
    }

    /// Renames agent `id` off the UI thread, and shows the new name in the list and on its page.
    pub fn rename_agent(&mut self, id: String, name: String, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        patchwork_ui_theme::off_thread(
            cx,
            move || core.rename_agent(&id, &name),
            move |this, renamed, cx| {
                match renamed {
                    Ok(agent) => {
                        if let Some(page) = &this.agent_page {
                            page.update(cx, |page, cx| page.show_saved(agent, cx));
                        }
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                this.reload_agents(cx);
            },
        );
    }

    /// Moves agent `id` out of the sidebar when `archived`, or back into it, off the UI thread.
    pub fn set_agent_archived(&mut self, id: &str, archived: bool, cx: &mut Context<Self>) {
        let (core, id) = (Arc::clone(&self.core), id.to_string());
        patchwork_ui_theme::off_thread(
            cx,
            move || core.set_agent_archived(&id, archived),
            move |this, done, cx| {
                if let Err(e) = done {
                    log::warn!(error:% = e; "could not archive or restore the agent: {{error}}");
                    this.error = Some(e.to_string());
                }
                this.reload_agents(cx);
            },
        );
    }

    /// Sends `text` and `attachments` to the chat of the agent shown, starting it with the
    /// composer's picks in `new` in the project picked on its page, and opens it. False when no agent is shown or a
    /// run is already starting, so the composer keeps the message.
    pub fn send_to_agent(
        &mut self,
        text: String,
        attachments: Vec<Attachment>,
        mut new: NewChat,
        cx: &mut Context<Self>,
    ) -> bool {
        let (Some(id), Some(page)) = (self.agent_shown.clone(), self.agent_page.clone()) else {
            return false;
        };
        if page.read(cx).starting {
            return false;
        }
        new.project_id = page.read(cx).run_place();
        page.update(cx, |p, cx| p.set_starting(true, cx));
        let (core, kept) = (Arc::clone(&self.core), (text.clone(), attachments.clone()));
        let task =
            cx.background_spawn(async move { core.run_agent_with(&id, new, &text, attachments) });
        cx.spawn(async move |this, cx| {
            let started = task.await;
            let _ = this.update(cx, |this, cx| {
                page.update(cx, |p, cx| p.set_starting(false, cx));
                match started {
                    Ok(chat) => {
                        this.reload_agents(cx);
                        // The run saved the picks it started with on the agent.
                        let saved = chat
                            .agent_id
                            .as_ref()
                            .and_then(|id| this.agents.iter().find(|b| &b.id == id))
                            .cloned();
                        page.update(cx, |p, cx| {
                            if let Some(agent) = saved {
                                p.show_saved(agent, cx);
                            }
                            p.load_usage(cx);
                        });
                        this.reload(cx);
                        this.open_chat(&chat.id, cx);
                    }
                    Err(e) => {
                        log::warn!(error:% = e; "could not start the agent's run: {{error}}");
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

    /// Opens Settings on Scheduled with a new routine that runs as `agent`.
    pub fn add_agent_routine(
        &mut self,
        agent: &Agent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings(Some("scheduled"), window, cx);
        if let Some(dialog) = self.settings_dialog(cx) {
            dialog.update(cx, |d, cx| d.add_routine(agent, cx));
        }
    }

    /// Asks, then deletes agent `id`. Its conversations stay, as chats.
    pub fn delete_agent(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .agents
            .iter()
            .find(|b| b.id == id)
            .map_or_else(|| "this agent".to_string(), |b| b.name.clone());
        let answer = window.prompt(
            PromptLevel::Warning,
            "Delete agent",
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
                .background_spawn(async move { core.remove_agent(&id) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = removed {
                    log::warn!(error:% = e; "could not delete the agent: {{error}}");
                    this.error = Some(e.to_string());
                }
                // Drops its page, and lists its conversations with the chats.
                this.reload_agents(cx);
                this.reload(cx);
            });
        })
        .detach();
    }
}
