//! Git's sign-in questions: a dialog over the window that takes a user name, a password drawn as
//! dots, or a yes or no, and hands the answer to the git command waiting on it. Escape declines.

// Rust guideline compliant 2026-10-09

use gpui::component::button::Button;
use gpui::component::button::ButtonVariants as _;
use gpui::component::Sizable as _;
use std::collections::VecDeque;

use gpui::{Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Window, div, prelude::*, px};
use patchwork_client::local::core::source_control::{CredentialKind, CredentialRequest};

use patchwork_design_editor::{Changed, Submitted, TextArea};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{font_mono, fs_md, radius_sm, theme};
use patchwork_design_components::dialog;

/// How wide the dialog is.
const WIDTH: f32 = 420.;

/// The question shown, with the field its answer is typed in.
struct Asked {
    request: CredentialRequest,
    field: Entity<TextArea>,
    focus: FocusHandle,
}

/// The last question was answered, so the window takes the focus back.
#[derive(Debug, Clone, Copy)]
pub struct Answered;

/// The question shown now, and those that arrived while it was.
#[derive(Default)]
pub struct CredentialDialog {
    asked: Option<Asked>,
    waiting: VecDeque<CredentialRequest>,
}

impl std::fmt::Debug for CredentialDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialDialog")
            .field("open", &self.asked.is_some())
            .field("waiting", &self.waiting.len())
            .finish()
    }
}

impl EventEmitter<Answered> for CredentialDialog {}

impl CredentialDialog {
    /// Whether a question is shown.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.asked.is_some()
    }

    /// Shows `request`, after the questions already waiting.
    pub fn ask(&mut self, request: CredentialRequest, window: &mut Window, cx: &mut Context<Self>) {
        if self.asked.is_some() {
            return self.waiting.push_back(request);
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
            this.answer(Some(value), window, cx);
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
        self.asked = Some(Asked {
            request,
            field,
            focus,
        });
        cx.notify();
    }

    /// Hands `value` to the command that asked, or with None declines, then shows the next
    /// question.
    fn answer(&mut self, value: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(asked) = self.asked.take() else {
            return;
        };
        // The field is dropped with what was typed in it.
        asked.field.update(cx, |field, cx| field.set_text("", cx));
        asked.request.answer(value);
        match self.waiting.pop_front() {
            Some(next) => self.ask(next, window, cx),
            None => cx.emit(Answered),
        }
        cx.notify();
    }
}

impl Render for CredentialDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(asked) = self.asked.as_ref() else {
            return div().into_any_element();
        };
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
        let decline = Button::new("credential-decline")
            .small()
            .label(no)
            .on_click(cx.listener(|this, _, window, cx| {
                // ssh takes the word, where git takes no answer as a refusal.
                let said = this
                    .asked
                    .as_ref()
                    .filter(|a| a.request.kind == CredentialKind::Confirm)
                    .map(|_| "no".to_string());
                this.answer(said, window, cx);
            }));
        let accept = Button::new("credential-accept")
            .primary()
            .small()
            .label(yes)
            .on_click(cx.listener(move |this, _, window, cx| {
                let value = if confirm {
                    "yes".to_string()
                } else {
                    field.read(cx).content.clone()
                };
                this.answer(Some(value), window, cx);
            }));
        let card = dialog::card("credential-dialog", title, &asked.focus, &t)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.answer(None, window, cx);
                }
            }))
            .flex()
            .flex_col()
            .gap(px(12.))
            .w(px(WIDTH))
            .p(px(16.))
            .text_size(px(fs_md()))
            .child(dialog::title_row(
                title,
                Button::new("credential-close")
                    .ghost()
                    .small()
                    .icon(IconName::X)
                    .accessibility_label("Cancel")
                    .on_click(cx.listener(|this, _, window, cx| this.answer(None, window, cx))),
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
                    .gap(px(8.))
                    .child(decline)
                    .child(accept),
            );
        dialog::backdrop("credential-backdrop", true)
            .items_center()
            .child(card)
            .into_any_element()
    }
}
