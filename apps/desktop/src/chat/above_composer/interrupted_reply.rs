//! A reply the app's closing cut off, or an error ended: the button that resumes its turn in the
//! agent's session. Patchwork resumes a cut-off turn itself at the next launch, once; the button
//! is for the turns it left alone. A reply a usage limit ended can also wait for the limit to
//! reset and resume then.

// Rust guideline compliant 2026-10-09

use patchwork_platform::now_ms;
use std::sync::Arc;

use gpui::{Div, Role, div, prelude::*, px};
use patchwork_backend::Core;
use patchwork_backend::chats::{Message, Chat};

use patchwork_ui_theme::{fs_sm, Theme};
use patchwork_ui_components::{BtnKind, btn};
use patchwork_ui_markdown::resets_in;
use crate::workspace::{Workspace, is_working};

impl Workspace {
    /// Resumes the turn of the open chat's cut-off reply.
    fn resume_turn(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.as_ref().filter(|t| !is_working(t)) else {
            return;
        };
        let (core, chat) = (Arc::clone(&self.core), chat.id.clone());
        patchwork_ui_theme::off_thread(
            cx,
            move || core.resume_turn(&chat),
            move |this, result, cx| {
                if let Err(e) = result {
                    this.error = Some(e.to_string());
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
            self.error = Some(e.to_string());
        }
        cx.notify();
    }
}

/// What resumes the turn of `m`, while it is the reply the chat can resume and no turn runs:
/// the Resume button, and for a reply a usage limit ended, resuming by itself once the limit
/// resets.
pub(in crate::chat) fn resume_row(
    core: &Core,
    chat: &Chat,
    m: &Message,
    t: &Theme,
    cx: &mut Context<Workspace>,
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
                btn(
                    "resume-turn",
                    "Resume this reply where it stopped",
                    BtnKind::Primary,
                    false,
                    t,
                )
                .child("Resume")
                .on_click(cx.listener(|w, _, _, cx| w.resume_turn(cx))),
            )
            .children(reset.map(|at| {
                let label = format!("Resume when the limit resets, {}", resets_in(at, now));
                btn("resume-at-reset", label.clone(), BtnKind::Plain, false, t)
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
                btn(
                    "resume-at-reset-cancel",
                    "Do not resume when the limit resets",
                    BtnKind::Plain,
                    false,
                    t,
                )
                .child("Cancel")
                .on_click(cx.listener(|w, _, _, cx| w.wait_for_limit(false, cx)))
            })),
    )
}
