//! Messages sent while the agent was busy, waiting for its turn to end
//! (`composer/components/QueuedMessages.tsx`, `hooks/useQueue.ts`). The agent has not seen them
//! yet, so each can be taken back to edit or dropped, steer the running turn, or be sent on.

use std::sync::Arc;
use std::time::Duration;

use gpui::{Context, Div, FontWeight, Role, Stateful, div, prelude::*, px};
use patchwork_server::threads::Message;

use crate::chat::goal::strip_action;
use crate::ui::theme::{FS_SM, FS_XS, RADIUS_SM, Theme};
use crate::ui::{button, icon, tooltip};
use crate::workspace::{Workspace, is_working};

/// A queued message being edited in the composer, and the draft the edit set aside.
pub struct QueuedEdit {
    chat: String,
    id: String,
    saved: String,
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

    fn composer_text(&self, cx: &Context<Self>) -> String {
        self.composer.read(cx).editor.read(cx).content.clone()
    }

    fn set_composer_text(&self, text: &str, cx: &mut Context<Self>) {
        let editor = self.composer.read(cx).editor.clone();
        editor.update(cx, |e, cx| e.set_text(text, cx));
    }

    /// Editing puts a queued message's text in the composer, setting the draft aside.
    fn edit_queued(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(thread) = &self.thread else {
            return;
        };
        let Some(text) = thread
            .messages
            .iter()
            .find(|m| m.queued && m.id == id)
            .map(|m| m.text.clone())
        else {
            return;
        };
        self.queued_edit = Some(QueuedEdit {
            chat: thread.id.clone(),
            id: id.to_string(),
            saved: self.composer_text(cx),
        });
        self.set_composer_text(&text, cx);
        cx.notify();
    }

    /// Alt+Up in an empty composer edits the latest queued message.
    pub fn edit_latest_queued(&mut self, cx: &mut Context<Self>) {
        if self.queued_edit.is_some() || !self.composer_text(cx).trim().is_empty() {
            return;
        }
        if let Some(id) = self.queued().last().map(|m| m.id.clone()) {
            self.edit_queued(&id, cx);
        }
    }

    /// Puts back the draft the edit set aside.
    fn cancel_queued_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = self.queued_edit.take() {
            self.set_composer_text(&edit.saved, cx);
            cx.notify();
        }
    }

    /// Sends the composer's text as the edited queued message's new text. Its attachments stay.
    pub fn update_queued(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(edit) = self.queued_edit.take() else {
            return;
        };
        let attachments = self
            .thread
            .as_ref()
            .and_then(|t| t.messages.iter().find(|m| m.id == edit.id))
            .map(|m| m.attachments.clone())
            .unwrap_or_default();
        match self
            .server
            .edit_queued(&edit.chat, &edit.id, text, attachments)
        {
            Ok(()) => self.set_composer_text(&edit.saved, cx),
            Err(e) => {
                self.error = Some(e);
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
            self.error = Some(e);
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
                    this.error = Some(e);
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
                    this.error = Some(e);
                }
                this.reload(cx);
            })
        })
        .detach();
    }

    /// A queued message can be sent while it is being edited: the agent took it, or it was
    /// steered. What was typed stays in the composer as an ordinary draft, after the draft the
    /// edit had set aside. Leaving the chat drops the edit and puts the draft back.
    fn settle_queued_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &self.queued_edit else {
            return;
        };
        let open = self.thread.as_ref().map(|t| t.id.as_str());
        let left = open != Some(edit.chat.as_str());
        if !left && self.queued().iter().any(|m| m.id == edit.id) {
            return;
        }
        // Deferred: the composer is not changed while the chat is drawn.
        cx.spawn(async move |this, cx| {
            let _ = this.update(cx, |this, cx| {
                let Some(edit) = this.queued_edit.take() else {
                    return;
                };
                let typed = this.composer_text(cx);
                let text = if left || edit.saved.trim().is_empty() {
                    if left { edit.saved } else { typed }
                } else {
                    format!("{}\n\n{typed}", edit.saved)
                };
                this.set_composer_text(&text, cx);
                cx.notify();
            });
        })
        .detach();
    }
}

fn icon_action(
    id: &'static str,
    label: &'static str,
    name: &'static str,
    t: &Theme,
) -> Stateful<Div> {
    let (fg, hover) = (t.text, t.bg_hover);
    button(id, label)
        .flex_none()
        .size(px(24.))
        .justify_center()
        .rounded(px(RADIUS_SM))
        .text_color(t.text_subtle)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon(name, 13.))
}

/// The queued strip, over the composer, while the chat has queued messages.
pub fn queued_strip(
    w: &mut Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<Stateful<Div>> {
    w.settle_queued_edit(cx);
    let thread = w.thread.as_ref()?;
    let first = thread.messages.iter().find(|m| m.queued)?;
    let working = is_working(thread);
    let open = w.queue_open;
    let editing = w.queued_edit.as_ref().map(|e| e.id.clone());
    let (muted, fg, hover) = (t.text_muted, t.text, t.bg_hover);
    let text = if first.text.is_empty() {
        "Attachments only".to_string()
    } else {
        first.text.clone()
    };
    let id = first.id.clone();
    let toggle = button("queue-toggle", "Queued message")
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
                .child("QUEUED MESSAGE"),
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
        .when(editing.is_some(), |d| {
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
    let can_edit = editing.is_none();
    let edit_tip = if editing.as_deref() == Some(id.as_str()) {
        "Editing this queued message"
    } else {
        "Edit queued message"
    };
    let pressed = w.steer_pressed;
    let row = div()
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
                .id("queued-text")
                .tooltip(tooltip(text.clone()))
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(t.text)
                .child(text),
        )
        .child({
            let edit = icon_action("queue-edit", "Edit queued message", "pencil-simple", t)
                .tooltip(tooltip(edit_tip));
            if can_edit {
                let id = id.clone();
                edit.on_click(cx.listener(move |w, _, _, cx| w.edit_queued(&id, cx)))
            } else {
                edit.opacity(0.4).cursor_default()
            }
        })
        .child({
            let steer = button("queue-steer", "Steer queued message")
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
            let steer = if pressed {
                steer.border_color(t.border).bg(hover)
            } else {
                steer.border_color(gpui::transparent_black())
            };
            if working {
                let id = id.clone();
                let border = t.border;
                steer
                    .hover(move |s| s.border_color(border).bg(hover))
                    .on_click(cx.listener(move |w, _, _, cx| w.steer_queued(&id, cx)))
            } else {
                steer
                    .opacity(0.4)
                    .text_color(t.text_subtle)
                    .cursor_default()
            }
        })
        .child(
            icon_action("queue-remove", "Remove queued message", "x", t)
                .tooltip(tooltip("Remove queued message"))
                .on_click(cx.listener(move |w, _, _, cx| w.remove_queued(&id, cx))),
        );
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
            .when(open, |d| d.child(row)),
    )
}
