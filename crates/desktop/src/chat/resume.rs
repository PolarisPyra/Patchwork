//! Asking before a large session whose prompt cache has expired is resumed: sending to it would re-read
//! all of its context at full price, so the composer first offers to compact it.

// Rust guideline compliant 2026-10-09

use gpui::{Div, div, prelude::*, px};
use patchwork_server::provider::AgentKind;
use patchwork_server::threads::{Role as MessageRole, Thread};

use crate::ui::theme::{fs_sm, radius, Theme};
use crate::ui::{BtnKind, btn, checkbox, compact_elapsed, format_tokens, now_ms};
use crate::workspace::{Workspace, is_working};

/// A prompt cache has expired by an hour of idling.
const COLD_AFTER_MS: u64 = 60 * 60 * 1000;

/// The prompt, asked in a thread after its last settled message. It goes once either moves on.
pub struct ResumeAsk {
    thread_id: String,
    after: Option<String>,
    dont_ask: bool,
}

/// The last message that is not waiting in the queue.
fn settled_id(thread: &Thread) -> Option<String> {
    thread
        .messages
        .iter()
        .rev()
        .find(|m| !m.queued)
        .map(|m| m.id.clone())
}

/// How long a Claude thread over `warn_above` has been idle, and its context, once its cache has
/// expired. None when its last reply was a `/compact`, whose usage still reports the context from
/// before it ran.
fn cold_large_session(thread: &Thread, now: u64, warn_above: u64) -> Option<(u64, u64)> {
    if thread.agent != AgentKind::Claude {
        return None;
    }
    let last = thread.messages.last()?;
    let usage = last.usage.as_ref()?;
    let finished = last.finished_at?;
    let last_user = thread
        .messages
        .iter()
        .rev()
        .find(|m| m.role == MessageRole::User);
    if last_user.is_some_and(|m| m.text.trim() == "/compact") {
        return None;
    }
    let idle = now.saturating_sub(finished);
    (idle >= COLD_AFTER_MS && usage.context > warn_above).then_some((idle, usage.context))
}

impl Workspace {
    /// Whether sending now should ask first; if so, asks.
    pub fn ask_before_resume(&mut self, cx: &mut Context<Self>) -> bool {
        let tokens = &self.settings.tokens;
        let Some(thread) = self.thread.as_ref().filter(|t| !is_working(t)) else {
            return false;
        };
        if !tokens.ask_before_resume
            || cold_large_session(thread, now_ms(), tokens.warn_above).is_none()
        {
            return false;
        }
        self.resume_ask = Some(ResumeAsk {
            thread_id: thread.id.clone(),
            after: settled_id(thread),
            dont_ask: false,
        });
        cx.notify();
        true
    }

    /// Compacts first, leaving the draft in the composer to send once that is done, or sends it.
    fn answer_resume(&mut self, compact: bool, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(ask) = self.resume_ask.take() else {
            return;
        };
        if ask.dont_ask {
            self.settings.tokens.ask_before_resume = false;
            if let Err(e) = self
                .server
                .store
                .update_settings(|s| s.tokens.ask_before_resume = false)
            {
                log::warn!(error:% = e; "could not save the resume setting: {{error}}");
            }
        }
        if compact {
            self.send_command("/compact", cx);
        } else {
            let composer = self.composer.clone();
            window.defer(cx, move |window, cx| {
                composer.update(cx, |c, cx| c.send_unasked(window, cx));
            });
        }
        cx.notify();
    }
}

pub fn resume_prompt(w: &Workspace, t: &Theme, cx: &mut Context<Workspace>) -> Option<Div> {
    let thread = w.thread.as_ref().filter(|t| !is_working(t))?;
    let ask = w
        .resume_ask
        .as_ref()
        .filter(|a| a.thread_id == thread.id && a.after == settled_id(thread))?;
    let (idle, context) = cold_large_session(thread, now_ms(), w.settings.tokens.warn_above)?;
    Some(
        div()
            .mb(px(6.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_panel)
            .p(px(10.))
            .text_size(px(fs_sm()))
            .text_color(t.text)
            .child(format!(
                "This session has been idle for {} and uses {} tokens. Compact before continuing?",
                compact_elapsed(idle),
                format_tokens(context)
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .child(
                        checkbox("resume-dont-ask", "Don't ask again", ask.dont_ask, t)
                            .text_color(t.text_muted)
                            .on_click(cx.listener(|w, _, _, cx| {
                                if let Some(ask) = w.resume_ask.as_mut() {
                                    ask.dont_ask = !ask.dont_ask;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(
                                btn("resume-continue", "Continue", BtnKind::Plain, false, t)
                                    .child("Continue")
                                    .on_click(cx.listener(|w, _, window, cx| {
                                        w.answer_resume(false, window, cx);
                                    })),
                            )
                            .child(
                                btn(
                                    "resume-compact",
                                    "Compact first",
                                    BtnKind::Primary,
                                    false,
                                    t,
                                )
                                .child("Compact first")
                                .on_click(cx.listener(
                                    |w, _, window, cx| {
                                        w.answer_resume(true, window, cx);
                                    },
                                )),
                            ),
                    ),
            ),
    )
}
