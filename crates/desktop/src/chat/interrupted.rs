//! A reply the app's closing cut off, or an error ended: the button that resumes its turn in the
//! agent's session. Patchwork resumes a cut-off turn itself at the next launch, once; the button
//! is for the turns it left alone. A reply a usage limit ended can also wait for the limit to
//! reset and resume then.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{Div, Role, div, prelude::*, px};
use patchwork_server::Server;
use patchwork_server::threads::{Message, Thread};

use crate::ui::theme::{fs_sm, Theme};
use crate::ui::{BtnKind, btn, now_ms, resets_in};
use crate::workspace::{Workspace, is_working};

impl Workspace {
    /// Resumes the turn of the open thread's cut-off reply.
    fn resume_turn(&mut self, cx: &mut Context<Self>) {
        let Some(thread) = self.thread.as_ref().filter(|t| !is_working(t)) else {
            return;
        };
        let (server, thread) = (Arc::clone(&self.server), thread.id.clone());
        let task = cx.background_spawn(async move { server.resume_turn(&thread) });
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

    /// Sets the open thread's failed turn to resume once its usage limit resets, or with
    /// `wait` off, drops that.
    fn wait_for_limit(&mut self, wait: bool, cx: &mut Context<Self>) {
        let Some(thread) = self.thread.as_ref() else {
            return;
        };
        let result = if wait {
            self.server.resume_at_limit_reset(&thread.id).map(|_| ())
        } else {
            self.server.cancel_limit_resume(&thread.id)
        };
        if let Err(e) = result {
            self.error = Some(e.to_string());
        }
        cx.notify();
    }
}

/// What resumes the turn of `m`, while it is the reply the thread can resume and no turn runs:
/// the Resume button, and for a reply a usage limit ended, resuming by itself once the limit
/// resets.
pub(super) fn resume_row(
    server: &Server,
    thread: &Thread,
    m: &Message,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<Div> {
    thread
        .resumable_reply()
        .filter(|last| last.id == m.id && !is_working(thread))?;
    let now = now_ms();
    let waiting = server.limit_resume_at(&thread.id);
    let reset = server.limit_reset(thread).filter(|_| waiting.is_none());
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
