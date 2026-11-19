//! Git's sign-in questions: a dialog over the window that takes a user name, a password drawn as
//! dots, or a yes or no, and hands the answer to the git command waiting on it. Escape declines.

// Rust guideline compliant 2026-10-09

use std::collections::VecDeque;

use gpui::{
    AnyElement, Entity, FocusHandle, Focusable, KeyDownEvent, Window, deferred, div, prelude::*, px,
};
use patchwork_core::workspace::{CredentialKind, CredentialRequest};

use super::Workspace;
use crate::ui::text_area::{Changed, Submitted, TextArea};
use crate::ui::theme::{font_mono, fs_md, radius_sm, theme};
use crate::ui::{BtnKind, btn, dialog, icon_button};

/// How wide the dialog is.
const WIDTH: f32 = 420.;

/// The question shown, with the field its answer is typed in.
struct Asked {
    request: CredentialRequest,
    field: Entity<TextArea>,
    focus: FocusHandle,
}

/// The question shown now, and those that arrived while it was.
#[derive(Default)]
pub struct Credentials {
    asked: Option<Asked>,
    waiting: VecDeque<CredentialRequest>,
}

impl Credentials {
    pub(super) fn is_open(&self) -> bool {
        self.asked.is_some()
    }
}

impl Workspace {
    /// Shows `request`, after the questions already waiting.
    pub fn ask_credential(
        &mut self,
        request: CredentialRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.credentials.asked.is_some() {
            return self.credentials.waiting.push_back(request);
        }
        let secret = request.kind == CredentialKind::Secret;
        let prompt = request.prompt.clone();
        let field = cx.new(|cx| {
            let mut field = TextArea::new("", prompt, cx);
            field.single_line = true;
            field.masked = secret;
            field
        });
        cx.subscribe_in(&field, window, |this, field, _: &Submitted, window, cx| {
            let value = field.read(cx).content.clone();
            this.answer_credential(Some(value), window, cx);
        })
        .detach();
        cx.subscribe(&field, |_, _, _: &Changed, cx| cx.notify())
            .detach();
        let focus = cx.focus_handle();
        if request.kind == CredentialKind::Confirm {
            window.focus(&focus, cx);
        } else {
            window.focus(&field.focus_handle(cx), cx);
        }
        self.credentials.asked = Some(Asked {
            request,
            field,
            focus,
        });
        cx.notify();
    }

    /// Hands `value` to the command that asked, or with None declines, then shows the next
    /// question.
    fn answer_credential(
        &mut self,
        value: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(asked) = self.credentials.asked.take() else {
            return;
        };
        // The field is dropped with what was typed in it.
        asked.field.update(cx, |field, cx| field.set_text("", cx));
        asked.request.answer(value);
        match self.credentials.waiting.pop_front() {
            Some(next) => self.ask_credential(next, window, cx),
            None => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    pub(super) fn credential_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let asked = self.credentials.asked.as_ref()?;
        let t = *theme(cx);
        let confirm = asked.request.kind == CredentialKind::Confirm;
        let title = if confirm {
            "Trust this host?"
        } else {
            "Sign in"
        };
        let (no, yes) = if confirm {
            ("No", "Yes")
        } else {
            ("Cancel", "Continue")
        };
        let field = asked.field.clone();
        let decline = btn("credential-decline", no, BtnKind::Plain, false, &t)
            .child(no)
            .on_click(cx.listener(|this, _, window, cx| {
                // ssh takes the word, where git takes no answer as a refusal.
                let said = this
                    .credentials
                    .asked
                    .as_ref()
                    .filter(|a| a.request.kind == CredentialKind::Confirm)
                    .map(|_| "no".to_string());
                this.answer_credential(said, window, cx);
            }));
        let accept = btn("credential-accept", yes, BtnKind::Primary, false, &t)
            .child(yes)
            .on_click(cx.listener(move |this, _, window, cx| {
                let value = if confirm {
                    "yes".to_string()
                } else {
                    field.read(cx).content.clone()
                };
                this.answer_credential(Some(value), window, cx);
            }));
        let card = dialog::card("credential-dialog", title, &asked.focus, &t)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.answer_credential(None, window, cx);
                }
            }))
            .flex()
            .flex_col()
            .gap(px(12.))
            .w(px(WIDTH))
            .p(px(14.))
            .text_size(px(fs_md()))
            .child(dialog::title_row(
                title,
                icon_button("credential-close", "Cancel", "x", 16., cx).on_click(
                    cx.listener(|this, _, window, cx| this.answer_credential(None, window, cx)),
                ),
            ))
            .child(
                div()
                    .text_color(t.text_muted)
                    .child(asked.request.prompt.clone()),
            )
            .when(!confirm, |card| {
                card.child(
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
                        .child(asked.field.clone()),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.))
                    .child(decline)
                    .child(accept),
            );
        Some(
            deferred(
                dialog::backdrop("credential-backdrop", true)
                    .items_center()
                    .child(card),
            )
            .with_priority(4)
            .into_any_element(),
        )
    }
}
