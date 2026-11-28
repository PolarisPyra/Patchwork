//! A reply the app's closing cut off, or an error ended: the button that resumes its turn in the
//! agent's session.
//!
//! Patchwork resumes a cut-off turn itself at the next launch, once; the button
//! is for the turns it left alone. A reply a usage limit ended can also wait for the limit to
//! reset and resume then.

// Rust guideline compliant 2026-11-27

use gpui::component::button::Button;
use gpui::component::button::ButtonVariants as _;
use gpui::component::Sizable as _;
use crate::prelude::*;
use patchwork_client::AppState;
use patchwork_platform::now_ms;
use std::sync::Arc;

use gpui::{Div, Role, div, prelude::*, px};
use patchwork_client::local::core::Core;
use patchwork_client::local::core::chats::{Message, Chat};

use patchwork_design_theme::{fs_sm, Theme};

use patchwork_design_text::resets_in;
use crate::window::{is_working};

/// The window's interrupted reply methods, for any [`ChatWindow`].
pub trait InterruptedReplyExt: ChatWindow {
    /// Resumes the turn of the open chat's cut-off reply.
    fn resume_turn(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.as_ref().filter(|t| !is_working(t)) else {
            return;
        };
        let (core, chat) = (Arc::clone(&self.core), chat.id.clone());
        patchwork_design_tasks::off_thread(
            cx,
            move || core.resume_turn(&chat),
            move |this, result, cx| {
                if let Err(e) = result {
                    AppState::set_error(cx, Some(e.to_string()));
                }
                this.reload(cx);
            },
        );
    }

    /// Sets the open chat's failed turn to resume once its usage limit resets, or with
    /// `wait` off, drops that.
    fn wait_for_limit(&mut self, wait: bool, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.as_ref() else {
            return;
        };
        let result = if wait {
            self.core.resume_at_limit_reset(&chat.id).map(|_| ())
        } else {
            self.core.cancel_limit_resume(&chat.id)
        };
        if let Err(e) = result {
            AppState::set_error(cx, Some(e.to_string()));
        }
        cx.notify();
    }
}

impl<W: ChatWindow> InterruptedReplyExt for W {}

/// What resumes the turn of `m`, while it is the reply the chat can resume and no turn runs:
/// the Resume button, and for a reply a usage limit ended, resuming by itself once the limit
/// resets.
pub fn resume_row<W: ChatWindow>(
    core: &Core,
    chat: &Chat,
    m: &Message,
    t: &Theme,
    cx: &mut Context<W>,
) -> Option<Div> {
    chat.resumable_reply()
        .filter(|last| last.id == m.id && !is_working(chat))?;
    let now = now_ms();
    let waiting = core.limit_resume_at(&chat.id);
    let reset = core.limit_reset(chat).filter(|_| waiting.is_none());
    Some(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                Button::new("resume-turn")
                    .primary()
                    .small()
                    .accessibility_label("Resume this reply where it stopped")
                    .child("Resume")
                    .on_click(cx.listener(|w, _, _, cx| w.resume_turn(cx))),
            )
            .children(reset.map(|at| {
                let label = format!("Resume when the limit resets, {}", resets_in(at, now));
                Button::new("resume-at-reset")
                    .small()
                    .accessibility_label(label.clone())
                    .child(label)
                    .on_click(cx.listener(|w, _, _, cx| w.wait_for_limit(true, cx)))
            }))
            .children(waiting.map(|at| {
                div()
                    .id("resume-waiting")
                    .role(Role::Status)
                    .text_size(px(fs_sm()))
                    .text_color(t.text_muted)
                    .child(format!("Resumes {}", resets_in(at, now)))
            }))
            .children(waiting.map(|_| {
                Button::new("resume-at-reset-cancel")
                    .small()
                    .accessibility_label("Do not resume when the limit resets")
                    .child("Cancel")
                    .on_click(cx.listener(|w, _, _, cx| w.wait_for_limit(false, cx)))
            })),
    )
}
