//! Opening chats, starting them, sending to them, and applying their agents' events.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::Duration;
use gpui::{Focusable, FollowMode, Window, prelude::*};
use patchwork_server::events::{AgentEvent, AgentEventEnvelope};
use patchwork_server::threads::{Attachment, NewThread, Part};
use super::state::{ReplyNotice, Workspace, end_thinking, message_mut};

impl Workspace {
    /// Makes a change to what is stored off the UI thread, which a write to the database would
    /// hold up, then shows what the store holds. `what` names the change in the log if it fails.
    pub fn change(
        &mut self,
        what: &'static str,
        change: impl FnOnce(&patchwork_server::Server) -> Result<(), patchwork_server::Error>
        + Send
        + 'static,
        cx: &mut Context<Self>,
    ) {
        let server = Arc::clone(&self.server);
        let task = cx.background_spawn(async move { change(&server) });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            this.update(cx, |this, cx| {
                if let Err(e) = done {
                    log::warn!(what, error:% = e; "could not {{what}}: {{error}}");
                }
                // A project's name is in the settings.
                this.settings = this.server.store.settings();
                this.reload(cx);
            })
        })
        .detach();
    }

    pub fn open_thread(&mut self, id: &str, cx: &mut Context<Self>) {
        match self.server.thread(id) {
            Ok(thread) => {
                let changed_project =
                    self.thread.as_ref().map(|t| &t.project_id) != Some(&thread.project_id);
                // Leaving a chat with its options still open counts as putting them away.
                if self.thread.as_ref().map(|t| t.id.as_str()) != Some(id)
                    && let Some(shown) = self.shown_choice.take()
                {
                    self.dismissed_choices.insert(shown);
                }
                self.remember_project(thread.project_id.clone());
                self.thread = Some(thread);
                self.open_folds.clear();
                self.open_steps.clear();
                self.open_agents.clear();
                let (server, last) = (Arc::clone(&self.server), id.to_string());
                cx.background_spawn(async move {
                    if let Err(e) = server.set_last_thread_id(&last) {
                        log::warn!(error:% = e; "could not record the open chat: {{error}}");
                    }
                })
                .detach();
                self.transcript_list.set_follow_mode(FollowMode::Tail);
                self.opened_at = std::time::Instant::now();
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(crate::chat::OPEN_HOLD).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
                if changed_project {
                    self.load_git(cx);
                }
            }
            Err(e) => log::warn!(id:%, error:% = e; "could not open chat {{id}}: {{error}}"),
        }
        cx.notify();
    }

    /// Opens `id` and sends it `text`, as the pull request's feedback does.
    pub fn send_to(&mut self, id: &str, text: String, cx: &mut Context<Self>) {
        self.reveal_thread(id, cx);
        let server = Arc::clone(&self.server);
        let id = id.to_string();
        let task =
            cx.background_spawn(async move { server.send_message(id, text, Vec::new(), true) });
        cx.spawn(async move |this, cx| {
            let sent = task.await;
            this.update(cx, |this, cx| {
                if let Err(e) = sent {
                    this.error = Some(e.to_string());
                }
                this.reload(cx);
            })
        })
        .detach();
    }

    /// Starts a task in the open project with `text` its first message, named `title`.
    pub fn start_task(&mut self, title: &str, text: String, cx: &mut Context<Self>) {
        let Some(project) = self.project().map(|p| p.id.clone()) else {
            return;
        };
        let new = self
            .composer
            .read(cx)
            .new_thread(Some(project), &self.settings);
        self.start_thread(new, title, text, cx);
    }

    /// Creates `new`, named `title`, opens it, and sends `text` as its first message.
    pub fn start_thread(
        &mut self,
        new: NewThread,
        title: &str,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let thread = match self.server.create_thread(new) {
            Ok(thread) => thread,
            Err(e) => {
                self.error = Some(e.to_string());
                return cx.notify();
            }
        };
        if let Err(e) = self.server.rename_thread(&thread.id, title) {
            log::warn!(error:% = e; "could not name the thread: {{error}}");
        }
        self.reload(cx);
        self.reveal_thread(&thread.id, cx);
        let server = Arc::clone(&self.server);
        let id = thread.id;
        let task =
            cx.background_spawn(async move { server.send_message(id, text, Vec::new(), false) });
        cx.spawn(async move |this, cx| {
            let sent = task.await;
            this.update(cx, |this, cx| {
                if let Err(e) = sent {
                    this.error = Some(e.to_string());
                }
                this.reload(cx);
            })
        })
        .detach();
    }

    /// Applies one event from an agent. Text streams in place; anything else reloads what the
    /// window shows from the store, which saved it before sending it.
    pub fn on_event(&mut self, envelope: AgentEventEnvelope, cx: &mut Context<Self>) {
        // A notice lasts until the agent's next output in that thread.
        let notice = match &envelope.event {
            AgentEvent::Retrying {
                message_id,
                reason,
                attempt,
                max_attempts,
            } => Some((
                message_id,
                ReplyNotice::Retrying(reason.clone(), *attempt, *max_attempts),
            )),
            AgentEvent::Quiet { message_id, since } => {
                Some((message_id, ReplyNotice::Quiet(*since)))
            }
            _ => None,
        };
        if let Some((message_id, notice)) = notice {
            self.notices
                .insert(message_id.clone(), (envelope.thread_id.clone(), notice));
            cx.notify();
            return;
        }
        if !self.notices.is_empty() {
            self.notices
                .retain(|_, (thread_id, _)| *thread_id != envelope.thread_id);
        }
        let open = self.thread.as_mut().filter(|t| t.id == envelope.thread_id);
        match envelope.event {
            AgentEvent::TextDelta { message_id, text } => {
                if let Some(message) = open.and_then(|t| message_mut(t, &message_id)) {
                    end_thinking(&mut message.parts);
                    message.text.push_str(&text);
                    match message.parts.last_mut() {
                        Some(Part::Text { text: last }) => last.push_str(&text),
                        _ => message.parts.push(Part::Text { text }),
                    }
                    cx.notify();
                }
            }
            AgentEvent::Thinking {
                message_id,
                text,
                started_at,
                ended_at,
            } => {
                if let Some(message) = open.and_then(|t| message_mut(t, &message_id)) {
                    match message.parts.last_mut() {
                        Some(Part::Thinking {
                            text: last,
                            ended_at: None,
                            ..
                        }) => last.push_str(&text),
                        _ => message.parts.push(Part::Thinking {
                            text,
                            started_at: started_at.unwrap_or_else(crate::ui::now_ms),
                            ended_at,
                        }),
                    }
                    cx.notify();
                }
            }
            _ => self.queue_reload(cx),
        }
    }

    /// Reloads the threads and the open chat once, however many events asked for it this frame.
    pub(super) fn queue_reload(&mut self, cx: &mut Context<Self>) {
        if self.reload_queued {
            return;
        }
        self.reload_queued = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                // One frame at 60 Hz, so a burst of changes reloads the list once.
                .timer(Duration::from_millis(16))
                .await;
            this.update(cx, |this, cx| {
                this.reload_queued = false;
                this.reload(cx);
            })
        })
        .detach();
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.threads = self.server.list_threads();
        self.folders = self.server.list_folders();
        if let Some(id) = self.thread.as_ref().map(|t| t.id.clone()) {
            match self.server.thread(&id) {
                Ok(thread) => self.thread = Some(thread),
                // A failed read keeps the chat on screen; a deleted chat is gone from the list.
                Err(e) if self.threads.iter().any(|t| t.id == id) => {
                    log::warn!(thread:% = id, error:% = e; "reloading thread {{thread}}: {{error}}");
                }
                Err(_) => self.thread = None,
            }
        }
        cx.notify();
    }

    pub fn archive(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.thread.as_ref().is_some_and(|t| t.id == id) {
            self.thread = None;
        }
        let id = id.to_string();
        self.change(
            "archive the chat",
            move |server| server.set_thread_archived(&id, true),
            cx,
        );
        cx.notify();
    }

    /// Sends the composer's text to the open chat, or starts `new` with it, in the role of
    /// `role`, when none is open.
    pub fn send(
        &mut self,
        text: String,
        attachments: Vec<Attachment>,
        new: Option<NewThread>,
        role: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.send_with(text, attachments, new, role, false, cx);
    }

    /// `send`, holding the message in the chat's queue for after the running turn when `queue`
    /// is set, rather than steering the turn with it.
    pub fn send_with(
        &mut self,
        text: String,
        attachments: Vec<Attachment>,
        new: Option<NewThread>,
        role: Option<String>,
        queue: bool,
        cx: &mut Context<Self>,
    ) {
        let id = match (&self.thread, new) {
            (Some(thread), _) => thread.id.clone(),
            (None, None) => return,
            (None, Some(new)) => match self.server.create_thread(new) {
                Ok(thread) => {
                    let id = thread.id.clone();
                    if role.is_some()
                        && let Err(e) = self.server.set_thread_role(&id, role)
                    {
                        log::warn!(error:% = e; "could not set the chat's agent: {{error}}");
                    }
                    self.thread = Some(thread);
                    id
                }
                Err(e) => {
                    log::warn!(error:% = e; "could not start a chat: {{error}}");
                    return;
                }
            },
        };
        let server = Arc::clone(&self.server);
        let draft = (id.clone(), text.clone(), attachments.clone());
        let task =
            cx.background_spawn(async move { server.send_message(id, text, attachments, queue) });
        cx.spawn(async move |this, cx| {
            let sent = task.await;
            this.update(cx, |this, cx| {
                this.reload(cx);
                if let Err(e) = sent {
                    log::warn!(error:% = e; "could not send the message: {{error}}");
                    this.error = Some(e.to_string());
                    // A message refused before it was stored goes back to the composer.
                    let (id, text, attachments) = draft;
                    let open = this.thread.as_ref().filter(|t| t.id == id);
                    if open.is_some_and(|t| !t.messages.iter().any(|m| m.text == text)) {
                        this.composer
                            .update(cx, |c, cx| c.put_back(&text, attachments, cx));
                    }
                }
                this.transcript_list.set_follow_mode(FollowMode::Tail);
            })
        })
        .detach();
    }

    /// Sends `command` to the open chat's agent, as the compact picker's Compact now does.
    pub fn send_command(&mut self, command: &str, cx: &mut Context<Self>) {
        let Some(id) = self.thread.as_ref().map(|t| t.id.clone()) else {
            return;
        };
        let server = Arc::clone(&self.server);
        let command = command.to_string();
        let task =
            cx.background_spawn(async move { server.send_message(id, command, Vec::new(), false) });
        cx.spawn(async move |this, cx| {
            let sent = task.await;
            this.update(cx, |this, cx| {
                if let Err(e) = sent {
                    log::warn!(error:% = e; "could not send the command: {{error}}");
                    this.error = Some(e.to_string());
                }
                this.reload(cx);
            })
        })
        .detach();
    }

    /// Starts a chat from the open one's history up to `message_id`, and opens it.
    pub fn fork(&mut self, thread_id: &str, message_id: &str, cx: &mut Context<Self>) {
        match self.server.fork_thread(thread_id, message_id) {
            Ok(thread) => {
                self.reload(cx);
                self.open_thread(&thread.id, cx);
            }
            Err(e) => log::warn!(error:% = e; "could not fork the chat: {{error}}"),
        }
    }

    /// Clears the chat pane so the next message starts a new chat.
    pub fn new_chat(&mut self, cx: &mut Context<Self>) {
        self.thread = None;
        cx.notify();
    }

    /// Starts a chat (no project) or a task in `project`, at the top of `folder` when given, shows
    /// it in the task list, opens it, and puts the cursor in the composer.
    pub fn new_thread_in(
        &mut self,
        project: Option<String>,
        folder: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new = self
            .composer
            .read(cx)
            .new_thread(project.clone(), &self.settings);
        let thread = match self.server.create_thread(new) {
            Ok(thread) => thread,
            Err(e) => return log::warn!(error:% = e; "could not start a chat: {{error}}"),
        };
        if let Some(folder) = folder {
            let ids: Vec<String> = std::iter::once(thread.id.clone())
                .chain(
                    self.threads
                        .iter()
                        .filter(|t| {
                            t.folder_id.as_deref() == Some(folder) && t.archived_at.is_none()
                        })
                        .map(|t| t.id.clone()),
                )
                .collect();
            if let Err(e) = self.server.reorder_threads(&ids, Some(folder)) {
                log::warn!(error:% = e; "could not put the task in its folder: {{error}}");
            }
        }
        self.remember_project(project);
        self.reload(cx);
        self.reveal_thread(&thread.id, cx);
        let editor = self.composer.read(cx).editor.clone();
        window.focus(&editor.read(cx).focus_handle(cx), cx);
        if self.server.asks_granular(thread.project_id.as_deref()) {
            let scope = super::GranularScope::Chat(thread.id);
            self.open_granular_dialog(scope, None, window, cx);
        }
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(thread) = &self.thread {
            self.server.cancel_turn(&thread.id);
        }
        cx.notify();
    }
}
