//! The agent's request for a secret. While it waits, a
//! field drawn as dots takes the value, which goes straight to the server and is never shown, kept
//! in the chat, or seen by the agent; once answered, a line with its outcome.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{AnyElement, Entity, FontWeight, Role, SharedString, div, prelude::*, px};
use patchwork_server::secrets::SecretStatus;
use patchwork_server::threads::{Approval, ApprovalStatus};

use crate::ui::text_area::{Changed, Submitted, TextArea};
use crate::ui::theme::{font_mono, FS_SM, radius, radius_sm, Theme, theme};
use crate::ui::{BtnKind, btn, icon};
use crate::workspace::Workspace;

/// The fields of the secrets the agent waits on, and the answers on their way, by request.
#[derive(Default)]
pub struct SecretState {
    fields: HashMap<String, Entity<TextArea>>,
    busy: HashSet<String>,
    errors: HashMap<String, String>,
}

fn outcome(status: SecretStatus) -> &'static str {
    match status {
        SecretStatus::Pending => "Waiting",
        SecretStatus::Saved => "Saved",
        SecretStatus::Declined => "Declined",
        SecretStatus::Cancelled => "Cancelled",
        SecretStatus::TimedOut => "Timed out",
    }
}

fn waiting(a: &Approval) -> bool {
    a.status == ApprovalStatus::Pending
        && a.secret
            .as_ref()
            .is_some_and(|s| s.status == SecretStatus::Pending)
}

impl Workspace {
    /// Gives each secret the open thread waits on a field, and drops the fields of the answered.
    pub fn sync_secret_fields(&mut self, cx: &mut Context<Self>) {
        let pending: Vec<(String, String, Option<String>)> = self
            .thread
            .iter()
            .flat_map(|t| &t.messages)
            .flat_map(|m| &m.approvals)
            .filter(|a| waiting(a))
            .map(|a| {
                let placeholder = a.secret.as_ref().and_then(|s| s.placeholder.clone());
                (a.id.clone(), a.title.clone(), placeholder)
            })
            .collect();
        self.secrets
            .fields
            .retain(|id, _| pending.iter().any(|(p, _, _)| p == id));
        let t = *theme(cx);
        for (id, title, placeholder) in pending {
            if self.secrets.fields.contains_key(&id) {
                continue;
            }
            let field = cx.new(|cx| {
                let mut field = TextArea::new(placeholder.unwrap_or_default(), title, cx);
                field.single_line = true;
                field.masked = true;
                field.text_color = t.text;
                field.placeholder_color = crate::ui::theme::input_placeholder();
                field.selection_color = t.accent.opacity(0.3);
                field
            });
            let key = id.clone();
            cx.subscribe(&field, move |this, field, _: &Submitted, cx| {
                let value = field.read(cx).content.clone();
                if !value.is_empty() {
                    this.answer_secret(key.clone(), Some(value), cx);
                }
            })
            .detach();
            cx.subscribe(&field, |_, _, _: &Changed, cx| cx.notify())
                .detach();
            self.secrets.fields.insert(id, field);
        }
    }

    /// Saves the value, or declines with None.
    fn answer_secret(&mut self, id: String, value: Option<String>, cx: &mut Context<Self>) {
        let Some(thread_id) = self.thread.as_ref().map(|t| t.id.clone()) else {
            return;
        };
        if !self.secrets.busy.insert(id.clone()) {
            return;
        }
        self.secrets.errors.remove(&id);
        let server = Arc::clone(&self.server);
        let request = id.clone();
        let task =
            cx.background_spawn(async move { server.answer_secret(&thread_id, &request, value) });
        cx.spawn(async move |this, cx| {
            let answered = task.await;
            let _ = this.update(cx, |this, cx| {
                match answered {
                    Ok(()) => {
                        if let Some(field) = this.secrets.fields.get(&id) {
                            field.update(cx, |f, cx| f.set_text("", cx));
                        }
                    }
                    Err(e) => {
                        this.secrets.busy.remove(&id);
                        this.secrets.errors.insert(id, e.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

pub fn secret_card(
    w: &Workspace,
    a: &Approval,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let status = a
        .secret
        .as_ref()
        .map_or(SecretStatus::Cancelled, |s| s.status);
    let field = w.secrets.fields.get(&a.id).filter(|_| waiting(a));
    let Some(field) = field else {
        let (name, color) = if status == SecretStatus::Saved {
            ("check-circle", t.success)
        } else {
            ("x-circle", t.danger)
        };
        return div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(px(6.))
            .px(px(4.))
            .text_size(px(FS_SM))
            .text_color(t.text_muted)
            .child(icon(name, 15.).flex_none().text_color(color))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(t.text)
                    .child(a.title.clone()),
            )
            .child(div().flex_none().child(format!("· {}", outcome(status))))
            .into_any_element();
    };
    let busy = w.secrets.busy.contains(&a.id);
    let empty = field.read(cx).content.is_empty();
    let id = a.id.clone();
    let decline = btn(
        SharedString::from(format!("secret-decline-{id}")),
        "Decline",
        BtnKind::Plain,
        busy,
        t,
    )
    .child("Decline");
    let save = btn(
        SharedString::from(format!("secret-save-{id}")),
        "Save securely",
        BtnKind::Primary,
        busy || empty,
        t,
    )
    .child("Save securely");
    div()
        .id(SharedString::from(format!("secret-{id}")))
        .role(Role::Form)
        .aria_label(a.title.clone())
        .flex()
        .flex_col()
        .gap(px(8.))
        .rounded(px(radius()))
        .border_1()
        .border_color(t.warning)
        .bg(t.bg_panel)
        .p(px(10.))
        .text_size(px(FS_SM))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_color(t.warning)
                .child(icon("key", 15.).flex_none())
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.text)
                        .child(a.title.clone()),
                ),
        )
        .when(!a.detail.is_empty(), |d| {
            d.child(div().text_color(t.text_muted).child(a.detail.clone()))
        })
        .child(
            div()
                .min_w_0()
                .h(px(28.))
                .flex()
                .items_center()
                .px(px(8.))
                .rounded(px(radius_sm()))
                .border_1()
                .border_color(t.border_strong)
                .bg(t.bg_input)
                .font_family(font_mono())
                .when(busy, |d| d.opacity(0.5))
                .child(field.clone()),
        )
        .children(w.secrets.errors.get(&id).map(|e| {
            div()
                .id(SharedString::from(format!("secret-error-{id}")))
                .role(Role::Alert)
                .text_color(t.danger)
                .child(e.clone())
        }))
        .child(
            div()
                .flex()
                .justify_end()
                .gap(px(6.))
                .child(if busy {
                    decline
                } else {
                    let id = id.clone();
                    decline.on_click(
                        cx.listener(move |w, _, _, cx| w.answer_secret(id.clone(), None, cx)),
                    )
                })
                .child(if busy || empty {
                    save
                } else {
                    let field = field.clone();
                    save.on_click(cx.listener(move |w, _, _, cx| {
                        let value = field.read(cx).content.clone();
                        w.answer_secret(id.clone(), Some(value), cx);
                    }))
                }),
        )
        .into_any_element()
}
