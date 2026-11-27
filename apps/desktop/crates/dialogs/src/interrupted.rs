//! The card that asks before resuming the replies the last session cut off, shown when Settings
//! says a launch asks first. It sits in the window's corner, under the update card's place.

// Rust guideline compliant 2026-10-09

use gpui::{EventEmitter, FontWeight, Role, Window, div, prelude::*, px};

use patchwork_ui_theme::{radius, radius_sm, theme};
use patchwork_ui_components::{BtnKind, btn, button, dialog_shadow};
use patchwork_ui_theme::icon;

/// The user asked to resume the interrupted replies.
#[derive(Debug, Clone, Copy)]
pub struct Resume;

/// How many replies wait to be resumed; the card shows while there are any.
#[derive(Debug, Default)]
pub struct InterruptedCard {
    waiting: usize,
}

impl EventEmitter<Resume> for InterruptedCard {}

impl InterruptedCard {
    /// Whether the card shows.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.waiting > 0
    }

    /// Says how many replies wait to be resumed, which shows the card, or with 0 hides it.
    pub fn set(&mut self, waiting: usize, cx: &mut Context<Self>) {
        self.waiting = waiting;
        cx.notify();
    }
}

impl Render for InterruptedCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = match self.waiting {
            0 => return div().into_any_element(),
            1 => "1 reply was interrupted".to_string(),
            n => format!("{n} replies were interrupted"),
        };
        let t = *theme(cx);
        let (hover, fg) = (t.bg_hover, t.text);
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
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_elevated)
            .p(px(10.))
            .shadow(dialog_shadow(&t))
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
                    &t,
                )
                .child("Resume")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set(0, cx);
                    cx.emit(Resume);
                })),
            )
            .child(
                button("dismiss-interrupted", "Dismiss")
                    .size(px(28.))
                    .flex_none()
                    .justify_center()
                    .rounded(px(radius_sm()))
                    .text_color(t.text_muted)
                    .hover(move |s| s.bg(hover).text_color(fg))
                    .child(icon("x", 14.))
                    .on_click(cx.listener(|this, _, _, cx| this.set(0, cx))),
            )
            .into_any_element()
    }
}
