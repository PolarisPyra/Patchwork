//! The card that asks before resuming the replies the last session cut off, shown when Settings
//! says a launch asks first.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{FontWeight, Role, deferred, div, prelude::*, px};

use super::state::Workspace;
use crate::ui::theme::{RADIUS, RADIUS_SM, Theme};
use crate::ui::{BtnKind, btn, button, icon, popup_shadow};

impl Workspace {
    /// Says how many replies wait to be resumed, which shows the card.
    pub fn set_interrupted(&mut self, waiting: usize, cx: &mut Context<Self>) {
        self.interrupted = waiting;
        cx.notify();
    }

    /// Resumes them all and closes the card.
    fn resume_interrupted(&mut self, cx: &mut Context<Self>) {
        self.interrupted = 0;
        let server = Arc::clone(&self.server);
        let task = cx.background_spawn(async move { server.resume_interrupted() });
        cx.spawn(async move |this, cx| {
            task.await;
            this.update(cx, Workspace::reload)
        })
        .detach();
        cx.notify();
    }

    /// The card, under the update card's place, while replies wait.
    pub(super) fn interrupted_card(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let label = match self.interrupted {
            0 => return None,
            1 => "1 reply was interrupted".to_string(),
            n => format!("{n} replies were interrupted"),
        };
        let (hover, fg) = (t.bg_hover, t.text);
        Some(
            deferred(
                div()
                    .id("interrupted-card")
                    .role(Role::Status)
                    .aria_label(label.clone())
                    .absolute()
                    .top(px(104.))
                    .right(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(t.border_strong)
                    .bg(t.bg_elevated)
                    .p(px(10.))
                    .shadow(popup_shadow(t))
                    .child(
                        div()
                            .min_w_0()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label),
                    )
                    .child(
                        btn(
                            "resume-interrupted",
                            "Resume the interrupted replies",
                            BtnKind::Primary,
                            false,
                            t,
                        )
                        .child("Resume")
                        .on_click(cx.listener(|this, _, _, cx| this.resume_interrupted(cx))),
                    )
                    .child(
                        button("dismiss-interrupted", "Dismiss")
                            .size(px(28.))
                            .flex_none()
                            .justify_center()
                            .rounded(px(RADIUS_SM))
                            .text_color(t.text_muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                            .child(icon("x", 14.))
                            .on_click(cx.listener(|this, _, _, cx| this.set_interrupted(0, cx))),
                    ),
            )
            .with_priority(1),
        )
    }
}
