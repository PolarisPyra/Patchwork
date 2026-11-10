//! Messages sent while the agent was busy, waiting for its turn to end. The agent has not seen them
//! yet, so each can be taken back to edit or dropped, steer the running turn, or be sent on.
//! Also what is written and attached in the composer, kept apart for each chat.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, Div, FontWeight, Role, SharedString, Stateful, div, prelude::*, px};
use patchwork_server::threads::{Attachment, Message};

use crate::chat::goal::strip_action;
use crate::ui::theme::{FS_SM, FS_XS, RADIUS_SM, Theme};
use crate::ui::{button, icon, tooltip};
use crate::workspace::{Workspace, is_working};

/// What is written and attached in the composer, unsent.
type Draft = (String, Vec<Attachment>);

/// A queued message being edited in the composer, and the draft the edit set aside.
pub struct QueuedEdit {
    chat: String,
    id: String,
    saved: Draft,
}

/// Each chat's draft while another chat is open, so the composer shows only the open chat's.
#[derive(Default)]
pub struct ChatDrafts {
    /// The chat the composer's draft belongs to: `None` is a chat not started yet.
    shown: Option<String>,
    kept: HashMap<Option<String>, Draft>,
}

impl Workspace {
    /// The open chat's queued messages, oldest first.
    pub fn queued(&self) -> Vec<&Message> {
        self.thread
            .as_ref()
            .map(|t| t.messages.iter().filter(|m| m.queued).collect())
            .unwrap_or_default()
    }

    pub fn editing_queued(&self) -> bool {
        self.queued_edit.is_some()
    }

    /// The composer's draft. The composer's own handlers defer what leads here: it cannot be
    /// read or changed from inside one of them.
    fn composer_draft(&self, cx: &App) -> Draft {
        self.composer.read(cx).draft(cx)
    }

    fn set_composer_draft(&self, draft: Draft, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |c, cx| c.set_draft(&draft.0, draft.1, cx));
    }

    /// The composer shows the open chat's draft: opening another chat, or starting one, keeps
    /// what was written and attached for the chat left and brings back the other's. An edit of a
    /// queued message is dropped on the way, and the draft it set aside is the one kept.
    pub fn sync_chat_draft(&mut self, cx: &mut Context<Self>) {
        let open = |w: &Self| w.thread.as_ref().map(|t| t.id.clone());
        if self.chat_drafts.shown == open(self) {
            return;
        }
        // Deferred: the composer is not changed while the chat is drawn.
        cx.spawn(async move |this, cx| {
            let _ = this.update(cx, |this, cx| {
                let now = open(this);
                if this.chat_drafts.shown == now {
                    return;
                }
                let left = match this.queued_edit.take() {
                    Some(edit) => edit.saved,
                    None => this.composer_draft(cx),
                };
                let was = std::mem::replace(&mut this.chat_drafts.shown, now.clone());
                if !left.0.trim().is_empty() || !left.1.is_empty() {
                    this.chat_drafts.kept.insert(was, left);
                }
                let draft = this.chat_drafts.kept.remove(&now).unwrap_or_default();
                this.set_composer_draft(draft, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Editing puts a queued message's text and attachments in the composer, setting the draft
    /// aside.
    fn edit_queued(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(thread) = &self.thread else {
            return;
        };
        let Some(message) = thread.messages.iter().find(|m| m.queued && m.id == id) else {
            return;
        };
        let draft = (message.text.clone(), message.attachments.clone());
        self.queued_edit = Some(QueuedEdit {
            chat: thread.id.clone(),
            id: id.to_string(),
            saved: self.composer_draft(cx),
        });
        self.set_composer_draft(draft, cx);
        cx.notify();
    }

    /// Alt+Up in an empty composer edits the latest queued message.
    pub fn edit_latest_queued(&mut self, cx: &mut Context<Self>) {
        let (text, attachments) = self.composer_draft(cx);
        if self.queued_edit.is_some() || !text.trim().is_empty() || !attachments.is_empty() {
            return;
        }
        if let Some(id) = self.queued().last().map(|m| m.id.clone()) {
            self.edit_queued(&id, cx);
        }
    }

    /// Puts back the draft the edit set aside.
    fn cancel_queued_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = self.queued_edit.take() {
            self.set_composer_draft(edit.saved, cx);
            cx.notify();
        }
    }

    /// Saves what the composer holds as the edited queued message: its text, and its attachments
    /// as they now are. The draft the edit set aside comes back.
    pub fn update_queued(&mut self, cx: &mut Context<Self>) {
        let (text, attachments) = self.composer_draft(cx);
        let text = text.trim().to_string();
        if text.is_empty() && attachments.is_empty() {
            return;
        }
        let Some(edit) = self.queued_edit.take() else {
            return;
        };
        match self
            .server
            .edit_queued(&edit.chat, &edit.id, text, attachments)
        {
            Ok(()) => self.set_composer_draft(edit.saved, cx),
            Err(e) => {
                self.error = Some(e.to_string());
                self.queued_edit = Some(edit);
            }
        }
        self.reload(cx);
    }

    /// Takes a queued message back from the agent's queue.
    fn remove_queued(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(thread) = self.thread.as_ref().map(|t| t.id.clone()) else {
            return;
        };
        if let Err(e) = self.server.remove_queued(&thread, id) {
            self.error = Some(e.to_string());
        }
        self.reload(cx);
    }

    fn steer_queued(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(thread) = self.thread.as_ref().filter(|t| is_working(t)) else {
            return;
        };
        let (server, thread, id) = (Arc::clone(&self.server), thread.id.clone(), id.to_string());
        let task = cx.background_spawn(async move { server.steer_queued(&thread, &id) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.error = Some(e.to_string());
                }
                this.reload(cx);
            })
        })
        .detach();
    }

    /// Ctrl+Enter in the composer steers with the first queued message, showing the Steer button
    /// pressed for a moment, as a click would.
    pub fn steer_first_queued(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.queued().first().map(|m| m.id.clone()) else {
            return;
        };
        self.steer_pressed = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                // Long enough for the pressed look to be seen.
                .timer(Duration::from_millis(150))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.steer_pressed = false;
                cx.notify();
            });
        })
        .detach();
        self.steer_queued(&id, cx);
    }

    /// Sends the queue on once no turn runs.
    fn resume_queued(&mut self, cx: &mut Context<Self>) {
        let Some(thread) = self.thread.as_ref().filter(|t| !is_working(t)) else {
            return;
        };
        let (server, thread) = (Arc::clone(&self.server), thread.id.clone());
        let task = cx.background_spawn(async move { server.resume_queued(&thread) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.error = Some(e.to_string());
                }
                this.reload(cx);
            })
        })
        .detach();
    }

    /// A queued message can be sent while it is being edited: the agent took it, or it was
    /// steered. What was typed and attached stays in the composer as an ordinary draft, after the
    /// draft the edit had set aside. Leaving the chat drops the edit, as `sync_chat_draft` does.
    fn settle_queued_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &self.queued_edit else {
            return;
        };
        let open = self.thread.as_ref().map(|t| t.id.as_str());
        if open != Some(edit.chat.as_str()) || self.queued().iter().any(|m| m.id == edit.id) {
            return;
        }
        // Deferred: the composer is not changed while the chat is drawn.
        cx.spawn(async move |this, cx| {
            let _ = this.update(cx, |this, cx| {
                let open = this.thread.as_ref().map(|t| t.id.as_str());
                let Some(edit) = this
                    .queued_edit
                    .take_if(|edit| open == Some(edit.chat.as_str()))
                else {
                    return;
                };
                let (typed, attached) = this.composer_draft(cx);
                let (mut text, mut attachments) = edit.saved;
                if text.trim().is_empty() {
                    text = typed;
                } else {
                    text = format!("{text}\n\n{typed}");
                }
                for attachment in attached {
                    if !attachments.iter().any(|a| a.path == attachment.path) {
                        attachments.push(attachment);
                    }
                }
                this.set_composer_draft((text, attachments), cx);
                cx.notify();
            });
        })
        .detach();
    }
}

fn icon_action(id: String, label: &'static str, name: &'static str, t: &Theme) -> Stateful<Div> {
    let (fg, hover) = (t.text, t.bg_hover);
    button(SharedString::from(id), label)
        .flex_none()
        .size(px(24.))
        .justify_center()
        .rounded(px(RADIUS_SM))
        .text_color(t.text_subtle)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon(name, 13.))
}

/// One queued message: its text, and the buttons to edit it, steer with it, or take it back.
/// `first` is the one Ctrl+Enter steers with, which shows its Steer button pressed.
fn queued_row(
    w: &Workspace,
    m: &Message,
    first: bool,
    working: bool,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let hover = t.bg_hover;
    let text = if m.text.is_empty() {
        "Attachments only".to_string()
    } else {
        m.text.clone()
    };
    let editing = w.queued_edit.as_ref().map(|e| e.id.as_str());
    let edit_tip = if editing == Some(m.id.as_str()) {
        "Editing this queued message"
    } else {
        "Edit queued message"
    };
    let edit = icon_action(
        format!("queue-edit-{}", m.id),
        "Edit queued message",
        "pencil-simple",
        t,
    )
    .tooltip(tooltip(edit_tip));
    let edit = if editing.is_none() {
        let id = m.id.clone();
        edit.on_click(cx.listener(move |w, _, _, cx| w.edit_queued(&id, cx)))
    } else {
        edit.opacity(0.4).cursor_default()
    };
    let steer = button(
        SharedString::from(format!("queue-steer-{}", m.id)),
        "Steer queued message",
    )
    .tooltip(tooltip("Send this queued message as a steer"))
    .flex_none()
    .h(px(24.))
    .gap(px(4.))
    .px(px(6.))
    .rounded(px(RADIUS_SM))
    .border_1()
    .text_size(px(FS_XS))
    .text_color(t.accent_text)
    .child(icon("arrow-right", 13.))
    .child("Steer");
    let steer = if first && w.steer_pressed {
        steer.border_color(t.border).bg(hover)
    } else {
        steer.border_color(gpui::transparent_black())
    };
    let steer = if working {
        let id = m.id.clone();
        let border = t.border;
        steer
            .hover(move |s| s.border_color(border).bg(hover))
            .on_click(cx.listener(move |w, _, _, cx| w.steer_queued(&id, cx)))
    } else {
        steer
            .opacity(0.4)
            .text_color(t.text_subtle)
            .cursor_default()
    };
    let id = m.id.clone();
    let remove = icon_action(
        format!("queue-remove-{}", m.id),
        "Remove queued message",
        "x",
        t,
    )
    .tooltip(tooltip("Remove queued message"))
    .on_click(cx.listener(move |w, _, _, cx| w.remove_queued(&id, cx)));
    div()
        .id(SharedString::from(format!("queued-{}", m.id)))
        .role(Role::ListItem)
        .flex()
        .min_w_0()
        .items_center()
        .gap(px(8.))
        .border_t_1()
        .border_color(t.border)
        .px(px(10.))
        .py(px(6.))
        .child(
            div()
                .id(SharedString::from(format!("queued-text-{}", m.id)))
                .tooltip(tooltip(text.clone()))
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(t.text)
                .child(text),
        )
        .child(edit)
        .child(steer)
        .child(remove)
}

/// The queued strip, over the composer, while the chat has queued messages: a row for each,
/// oldest first, which is the order they are sent in.
pub fn queued_strip(
    w: &mut Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<Stateful<Div>> {
    w.settle_queued_edit(cx);
    let thread = w.thread.as_ref()?;
    let queued = w.queued();
    if queued.is_empty() {
        return None;
    }
    let working = is_working(thread);
    let open = w.queue_open;
    let editing = w.queued_edit.is_some();
    let (muted, fg) = (t.text_muted, t.text);
    let title = if queued.len() == 1 {
        "QUEUED MESSAGE".to_string()
    } else {
        format!("{} QUEUED MESSAGES", queued.len())
    };
    let rows: Vec<Stateful<Div>> = queued
        .iter()
        .enumerate()
        .map(|(i, m)| queued_row(w, m, i == 0, working, t, cx))
        .collect();
    let toggle = button("queue-toggle", "Queued messages")
        .aria_expanded(open)
        .flex_1()
        .min_w_0()
        .gap(px(8.))
        .text_color(muted)
        .hover(move |s| s.text_color(fg))
        .child(
            icon("list-bullets", 13.)
                .flex_none()
                .text_color(t.text_subtle),
        )
        .child(
            div()
                .text_size(px(FS_XS))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            icon(if open { "caret-down" } else { "caret-right" }, 12.)
                .ml_auto()
                .flex_none(),
        )
        .on_click(cx.listener(|w, _, _, cx| {
            w.queue_open = !w.queue_open;
            cx.notify();
        }));
    let actions = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .when(editing, |d| {
            d.child(
                strip_action("queue-cancel-edit", "Cancel edit", t)
                    .on_click(cx.listener(|w, _, _, cx| w.cancel_queued_edit(cx))),
            )
        })
        .when(!working, |d| {
            d.child(
                strip_action("queue-send", "Send", t)
                    .on_click(cx.listener(|w, _, _, cx| w.resume_queued(cx))),
            )
        });
    Some(
        div()
            .id("queued")
            .role(Role::Region)
            .aria_label("Queued messages")
            .mx(px(8.))
            .mb(px(6.))
            .overflow_hidden()
            .rounded(px(RADIUS_SM))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_panel)
            .text_size(px(FS_SM))
            .shadow_sm()
            .child(
                div()
                    .flex()
                    .h(px(36.))
                    .min_w_0()
                    .items_center()
                    .gap(px(8.))
                    .bg(t.bg_app)
                    .px(px(10.))
                    .child(toggle)
                    .child(actions),
            )
            .when(open, |d| {
                d.child(
                    div()
                        .id("queued-rows")
                        .role(Role::List)
                        .aria_label("Queued messages")
                        // About four rows, then the list scrolls.
                        .max_h(px(148.))
                        .overflow_y_scroll()
                        .children(rows),
                )
            }),
    )
}
