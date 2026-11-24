//! Opening chats, starting them, sending to them, and applying their agents' events.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::Duration;
use gpui::{Focusable, FollowMode, Window, prelude::*};
use patchwork_core::events::{AgentEvent, AgentEventEnvelope, Place as Shown};
use patchwork_core::chats::{Attachment, LastPlace, NewChat, Part};
use crate::workspace::{Place, ReplyNotice, Workspace, end_thinking, message_mut};

impl Workspace {
    /// Makes a change to what is stored off the UI thread, which a write to the database would
    /// hold up, then shows what the store holds. `what` names the change in the log if it fails.
    pub fn change(
        &mut self,
        what: &'static str,
        change: impl FnOnce(&patchwork_core::Core) -> Result<(), patchwork_core::Error> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let core = Arc::clone(&self.core);
        patchwork_ui::off_thread(
            cx,
            move || change(&core),
            move |this, done, cx| {
                if let Err(e) = done {
                    log::warn!(what, error:% = e; "could not {{what}}: {{error}}");
                }
                // A project's name is in the settings.
                this.settings = this.core.settings();
                this.reload(cx);
            },
        );
    }

    /// What the window shows now, as a place to come back to.
    pub(crate) fn place(&self) -> Option<Place> {
        match self.page {
            crate::workspace::Page::Orchestrator => Some(Place::Orchestrator),
            crate::workspace::Page::Quilts => Some(Place::Quilts(self.quilt_open.clone())),
            crate::workspace::Page::Chats if self.agent_shown.is_some() => {
                self.agent_shown.clone().map(Place::Agent)
            }
            crate::workspace::Page::Chats => self.chat.as_ref().map(|t| {
                if self.peeking {
                    Place::Peek(t.id.clone())
                } else {
                    Place::Chat(t.id.clone())
                }
            }),
        }
    }

    /// What fills the window as the next launch opens it: a chat looked in on opens as a chat.
    fn last_place(&self) -> Option<LastPlace> {
        Some(match self.place()? {
            Place::Orchestrator => LastPlace::Orchestrator,
            Place::Chat(id) | Place::Peek(id) => LastPlace::Chat(id),
            Place::Agent(id) => LastPlace::Agent(id),
            Place::Quilts(_) => return None,
        })
    }

    /// Saves what fills the window, off the UI thread, when it changed since it was last saved.
    pub(crate) fn remember_place(&mut self, cx: &mut Context<Self>) {
        let Some(place) = self.last_place() else {
            return;
        };
        if self.saved_place.as_ref() == Some(&place) {
            return;
        }
        self.saved_place = Some(place.clone());
        let core = Arc::clone(&self.core);
        cx.background_spawn(async move {
            if let Err(e) = core.set_last_place(&place) {
                log::warn!(error:% = e; "could not record what the window shows: {{error}}");
            }
        })
        .detach();
    }

    /// Opens what filled the window when the app last closed, else the orchestrator page, with
    /// the last chat ready behind it.
    pub(crate) fn restore_place(&mut self, cx: &mut Context<Self>) {
        let shown = match self.core.last_place() {
            Some(LastPlace::Chat(id)) if self.chats.iter().any(|t| t.id == id) => {
                self.show(&Place::Chat(id), cx)
            }
            Some(LastPlace::Agent(id)) => self.show(&Place::Agent(id), cx),
            Some(LastPlace::Chat(_) | LastPlace::Orchestrator) | None => false,
        };
        if !shown {
            self.page = crate::workspace::Page::Orchestrator;
        }
        self.saved_place = self.last_place();
    }

    pub fn open_chat(&mut self, id: &str, cx: &mut Context<Self>) {
        self.go_to(&Place::Chat(id.to_string()), cx);
    }

    /// Opens chat `id` read-only with the sidebar closed, as a subagent shows, for looking in on
    /// it from the orchestrator page.
    pub fn peek_chat(&mut self, id: &str, cx: &mut Context<Self>) {
        self.go_to(&Place::Peek(id.to_string()), cx);
    }

    pub(crate) fn go_to(&mut self, to: &Place, cx: &mut Context<Self>) {
        let from = self.place();
        if self.show(to, cx) {
            self.went_from(from);
        }
    }

    /// Records `from` for the back button, unless the window still shows it.
    pub(crate) fn went_from(&mut self, from: Option<Place>) {
        if let Some(from) = from.filter(|from| Some(from) != self.place().as_ref()) {
            self.history_back.push(from);
            self.history_forward.clear();
        }
    }

    /// Whether there is a place to go back to.
    pub fn can_go_back(&self) -> bool {
        !self.history_back.is_empty()
    }

    /// Shows what was shown before this, as a browser's back button does.
    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        self.step_history(true, cx);
    }

    /// Goes back, or to the orchestrator page when there is nothing to go back to.
    pub fn back_to_orchestrator(&mut self, cx: &mut Context<Self>) {
        if self.can_go_back() {
            self.go_back(cx);
        } else {
            self.go_to(&Place::Orchestrator, cx);
        }
    }

    /// Shows what was gone back from.
    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        self.step_history(false, cx);
    }

    /// Steps one place back or forward, skipping any chat deleted since.
    fn step_history(&mut self, back: bool, cx: &mut Context<Self>) {
        let from = self.place();
        loop {
            let next = if back {
                self.history_back.pop()
            } else {
                self.history_forward.pop()
            };
            let Some(next) = next else {
                return;
            };
            if self.show(&next, cx) {
                if let Some(from) = from {
                    if back {
                        self.history_forward.push(from);
                    } else {
                        self.history_back.push(from);
                    }
                }
                return;
            }
        }
    }

    /// Shows `place`, and whether it could be shown.
    fn show(&mut self, place: &Place, cx: &mut Context<Self>) -> bool {
        match place {
            Place::Orchestrator => {
                self.page = crate::workspace::Page::Orchestrator;
                cx.notify();
                true
            }
            Place::Chat(id) => self.show_chat(id, cx),
            Place::Agent(id) => self.show_agent(id, cx),
            Place::Quilts(open) => self.show_quilts(open.as_ref(), cx),
            Place::Peek(id) => {
                let shown = self.show_chat(id, cx);
                self.peeking |= shown;
                shown
            }
        }
    }

    /// Shows chat `id`, and whether it could be opened.
    pub(crate) fn show_chat(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let opened = match self.core.chat(id) {
            Ok(chat) => {
                // A chat opened from the orchestrator page is shown in place of the page.
                self.page = crate::workspace::Page::Chats;
                self.peeking = false;
                self.agent_shown = None;
                let changed_project =
                    self.chat.as_ref().map(|t| &t.project_id) != Some(&chat.project_id);
                // Leaving a chat with its options still open counts as putting them away.
                if self.chat.as_ref().map(|t| t.id.as_str()) != Some(id)
                    && let Some(shown) = self.shown_choice.take()
                {
                    self.dismissed_choices.insert(shown);
                }
                self.remember_project(chat.project_id.clone());
                if self.core.in_cli(id) {
                    self.in_cli.insert(id.to_owned());
                }
                self.chat = Some(chat);
                self.open_folds.clear();
                self.open_steps.clear();
                self.open_agents.clear();
                let (core, last) = (Arc::clone(&self.core), id.to_string());
                cx.background_spawn(async move {
                    if let Err(e) = core.set_last_chat_id(&last) {
                        log::warn!(error:% = e; "could not record the open chat: {{error}}");
                    }
                })
                .detach();
                self.transcript_list.set_follow_mode(FollowMode::Tail);
                self.opened_at = std::time::Instant::now();
                patchwork_ui::after(cx, super::OPEN_HOLD, |_, cx| cx.notify());
                if changed_project {
                    self.load_git(cx);
                } else {
                    // Another task of the project may work in a worktree of its own.
                    self.read_git(false, cx);
                }
                self.load_worktree(cx);
                // A task known to have a worktree syncs the terminal once its worktree root loads,
                // so switching to it does not open a shell in the project folder first.
                if self.chat_folder().is_some() || !self.worktree_marks.contains_key(id) {
                    self.sync_panel(cx);
                }
                true
            }
            Err(e) => {
                log::warn!(id:%, error:% = e; "could not open chat {{id}}: {{error}}");
                false
            }
        };
        cx.notify();
        opened
    }

    /// Opens `id` and sends it `text`, as the pull request's feedback does.
    pub fn send_to(&mut self, id: &str, text: String, cx: &mut Context<Self>) {
        self.reveal_chat(id, cx);
        let core = Arc::clone(&self.core);
        let id = id.to_string();
        let task =
            cx.background_spawn(async move { core.send_message(id, text, Vec::new(), true) });
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
            .new_chat(Some(project), &self.settings);
        self.start_chat(new, title, text, cx);
    }

    /// Creates `new`, named `title`, opens it, and sends `text` as its first message.
    pub fn start_chat(&mut self, new: NewChat, title: &str, text: String, cx: &mut Context<Self>) {
        let chat = match self.core.create_chat(new) {
            Ok(chat) => chat,
            Err(e) => {
                self.error = Some(e.to_string());
                return cx.notify();
            }
        };
        if let Err(e) = self.core.rename_chat(&chat.id, title) {
            log::warn!(error:% = e; "could not name the chat: {{error}}");
        }
        self.reload(cx);
        self.reveal_chat(&chat.id, cx);
        let core = Arc::clone(&self.core);
        let id = chat.id;
        let task =
            cx.background_spawn(async move { core.send_message(id, text, Vec::new(), false) });
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
    pub fn on_event(
        &mut self,
        envelope: AgentEventEnvelope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The system notification's tag starts with its chat, so a click on it opens that chat.
        match &envelope.event {
            AgentEvent::Notice { notice } => {
                cx.show_system_notification(gpui::SystemNotification {
                    tag: format!("{}|{}", envelope.chat_id, notice.tag).into(),
                    title: notice.title.clone().into(),
                    body: notice.body.clone().into(),
                    actions: Vec::new(),
                });
                return;
            }
            AgentEvent::ApprovalResolved { approval_id, .. } => {
                cx.dismiss_system_notification(&format!("{}|{approval_id}", envelope.chat_id));
            }
            AgentEvent::QuiltsChanged => {
                self.load_quilt_list(cx);
                if let Some(page) = &self.quilts {
                    page.update(cx, patchwork_pages::quilts::QuiltsPage::reload);
                }
                return;
            }
            // Changed from a chat app: what Settings does when it changes them.
            AgentEvent::SettingsChanged => {
                self.settings = self.core.settings();
                self.backgrounds.reload();
                self.sync_panel(cx);
                self.apply_theme(window, cx);
                self.reload(cx);
                return;
            }
            // The core checked that nothing is lost by it.
            AgentEvent::RestartToUpdate => {
                crate::workspace::restart(cx);
                return;
            }
            AgentEvent::OpenChat => {
                self.reload(cx);
                self.reveal_chat(&envelope.chat_id, cx);
                window.activate_window();
                return;
            }
            AgentEvent::Show { place } => {
                self.reload(cx);
                match place {
                    Shown::Project { id } => self.select_project(id, cx),
                    Shown::File { project_id, path } => {
                        if self.settings.active_project_id.as_deref() != Some(project_id) {
                            self.select_project(project_id, cx);
                        }
                        self.open_change(path, window, cx);
                    }
                    Shown::Settings { section } => {
                        // Settings that is open already moves to the section asked for.
                        self.settings_dialog = None;
                        self.open_settings(section.as_deref(), window, cx);
                    }
                }
                window.activate_window();
                return;
            }
            // A finished reply adds to what Costs counts, and Source Control shows where its
            // files ended, whether or not the file watcher saw every change.
            AgentEvent::TurnCompleted { .. } => {
                if self.chat.as_ref().is_some_and(|t| t.id == envelope.chat_id) {
                    self.read_git(false, cx);
                }
                if let Some(dialog) = &self.settings_dialog {
                    dialog.update(
                        cx,
                        patchwork_modals::settings::SettingsDialog::refresh_costs,
                    );
                }
            }
            _ => {}
        }
        // A notice lasts until the agent's next output in that chat.
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
                .insert(message_id.clone(), (envelope.chat_id.clone(), notice));
            cx.notify();
            return;
        }
        if !self.notices.is_empty() {
            self.notices
                .retain(|_, (chat_id, _)| *chat_id != envelope.chat_id);
        }
        let open = self.chat.as_mut().filter(|t| t.id == envelope.chat_id);
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
                            started_at: started_at.unwrap_or_else(patchwork_ui::now_ms),
                            ended_at,
                        }),
                    }
                    cx.notify();
                }
            }
            _ => self.queue_reload(cx),
        }
    }

    /// Reloads the chats and the open chat once, however many events asked for it this frame.
    pub(crate) fn queue_reload(&mut self, cx: &mut Context<Self>) {
        if self.reload_queued {
            return;
        }
        self.reload_queued = true;
        // One frame at 60 Hz, so a burst of changes reloads the list once.
        patchwork_ui::after(cx, Duration::from_millis(16), |this, cx| {
            this.reload_queued = false;
            this.reload(cx);
        });
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let chats = self.core.list_chats();
        self.folders = self.core.list_folders();
        self.chat_apps = self.core.chat_chat_apps();
        self.chat_app_status = self.core.gateway_status();
        // A chat app's first message makes its agent, and an agent's new chat may be its chat.
        let changed = chats
            .iter()
            .any(|t| t.agent_id.is_some() && !self.chats.iter().any(|known| known.id == t.id));
        self.chats = chats;
        if changed
            || self.chats.iter().any(|t| {
                t.agent_id
                    .as_ref()
                    .is_some_and(|id| !self.agents.iter().any(|b| &b.id == id))
            })
        {
            self.reload_agents(cx);
        }
        if let Some(id) = self.chat.as_ref().map(|t| t.id.clone()) {
            match self.core.chat(&id) {
                Ok(chat) => self.chat = Some(chat),
                // A failed read keeps the chat on screen; a deleted chat is gone from the list.
                Err(e) if self.chats.iter().any(|t| t.id == id) => {
                    log::warn!(chat:% = id, error:% = e; "reloading chat {{chat}}: {{error}}");
                }
                Err(_) => self.close_chat(cx),
            }
        }
        self.load_worktree_marks(cx);
        cx.notify();
    }

    pub fn archive(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.chat.as_ref().is_some_and(|t| t.id == id) {
            self.close_chat(cx);
        }
        let id = id.to_string();
        self.change(
            "archive the chat",
            move |core| core.set_chat_archived(&id, true),
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
        new: Option<NewChat>,
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
        new: Option<NewChat>,
        role: Option<String>,
        queue: bool,
        cx: &mut Context<Self>,
    ) {
        let id = match (&self.chat, new) {
            (Some(chat), _) => chat.id.clone(),
            (None, None) => return,
            (None, Some(new)) => match self.core.create_chat(new) {
                Ok(chat) => {
                    let id = chat.id.clone();
                    if role.is_some()
                        && let Err(e) = self.core.set_chat_role(&id, role)
                    {
                        log::warn!(error:% = e; "could not set the chat's agent: {{error}}");
                    }
                    self.chat = Some(chat);
                    id
                }
                Err(e) => {
                    log::warn!(error:% = e; "could not start a chat: {{error}}");
                    return;
                }
            },
        };
        // Sending brings a chat carried on in its CLI back first.
        self.in_cli.remove(&id);
        let core = Arc::clone(&self.core);
        let draft = (id.clone(), text.clone(), attachments.clone());
        let task =
            cx.background_spawn(async move { core.send_message(id, text, attachments, queue) });
        cx.spawn(async move |this, cx| {
            let sent = task.await;
            this.update(cx, |this, cx| {
                this.reload(cx);
                if let Err(e) = sent {
                    log::warn!(error:% = e; "could not send the message: {{error}}");
                    this.error = Some(e.to_string());
                    // A message refused before it was stored goes back to the composer.
                    let (id, text, attachments) = draft;
                    let open = this.chat.as_ref().filter(|t| t.id == id);
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
        let Some(id) = self.chat.as_ref().map(|t| t.id.clone()) else {
            return;
        };
        let core = Arc::clone(&self.core);
        let command = command.to_string();
        let task =
            cx.background_spawn(async move { core.send_message(id, command, Vec::new(), false) });
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

    /// Starts a chat from the open one's history up to `message_id`, and opens it. A task's fork
    /// works in its worktree, or with `new_worktree`, in one of its own; None follows the
    /// project's setting.
    pub fn fork(
        &mut self,
        chat_id: &str,
        message_id: &str,
        new_worktree: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        let core = Arc::clone(&self.core);
        let (chat_id, message_id) = (chat_id.to_owned(), message_id.to_owned());
        let task =
            cx.background_spawn(async move { core.fork_chat(&chat_id, &message_id, new_worktree) });
        cx.spawn(async move |this, cx| {
            let forked = task.await;
            this.update(cx, |this, cx| match forked {
                Ok(chat) => {
                    this.reload(cx);
                    this.open_chat(&chat.id, cx);
                }
                Err(e) => {
                    log::warn!(error:% = e; "could not fork the chat: {{error}}");
                    this.error = Some(e.to_string());
                    cx.notify();
                }
            })
        })
        .detach();
    }

    /// Clears the chat pane so the next message starts a new chat.
    pub fn new_chat(&mut self, cx: &mut Context<Self>) {
        self.close_chat(cx);
        cx.notify();
    }

    /// Closes the open chat, and points Source Control back at the project's folder when it
    /// showed the chat's worktree.
    pub(crate) fn close_chat(&mut self, cx: &mut Context<Self>) {
        self.chat = None;
        let folder = self.project().map(|p| p.path.clone());
        if self.git_root.is_some() && self.git_root != folder {
            self.read_git(false, cx);
        }
        self.sync_panel(cx);
    }

    /// Starts a chat (no project) or a task in `project`, at the top of `folder` when given, shows
    /// it in the task list, opens it, and puts the cursor in the composer.
    pub fn new_chat_in(
        &mut self,
        project: Option<String>,
        folder: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new = self
            .composer
            .read(cx)
            .new_chat(project.clone(), &self.settings);
        let chat = match self.core.create_chat(new) {
            Ok(chat) => chat,
            Err(e) => return log::warn!(error:% = e; "could not start a chat: {{error}}"),
        };
        if let Some(folder) = folder {
            let ids: Vec<String> = std::iter::once(chat.id.clone())
                .chain(
                    self.chats
                        .iter()
                        .filter(|t| {
                            t.folder_id.as_deref() == Some(folder) && t.archived_at.is_none()
                        })
                        .map(|t| t.id.clone()),
                )
                .collect();
            if let Err(e) = self.core.reorder_chats(&ids, Some(folder)) {
                log::warn!(error:% = e; "could not put the task in its folder: {{error}}");
            }
        }
        self.remember_project(project);
        self.reload(cx);
        self.reveal_chat(&chat.id, cx);
        let editor = self.composer.read(cx).editor.clone();
        window.focus(&editor.read(cx).focus_handle(cx), cx);
        if self.core.asks_granular(chat.project_id.as_deref()) {
            let scope = crate::workspace::GranularScope::Chat(chat.id);
            self.open_granular_dialog(scope, None, window, cx);
        }
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(chat) = &self.chat {
            self.core.cancel_turn(&chat.id);
        }
        cx.notify();
    }
}
