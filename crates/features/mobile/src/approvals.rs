//! The approvals agents wait on, in every chat, each with Allow and Deny.

// Rust guideline compliant 2026-11-27

use gpui::component::button::Button;
use gpui::component::button::ButtonVariants as _;
use gpui::component::Sizable as _;
use gpui::{
    App, Context, EventEmitter, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use patchwork_client::{PendingApproval, Session};

use patchwork_design_theme::{Theme, fs_sm, theme};

use crate::{back_button, top_bar};

/// What the approvals screen asks the app for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalsEvent {
    /// Go back to the chat list.
    Back,
}

/// The approvals waiting, drawn from [`Session`].
#[derive(Debug)]
pub struct ApprovalsScreen;

impl ApprovalsScreen {
    /// Makes the screen, which draws again whenever the session changes.
    #[must_use]
    pub fn new(cx: &mut Context<'_, Self>) -> Self {
        cx.observe_global::<Session>(|_, cx| cx.notify()).detach();
        Self
    }
}

impl EventEmitter<ApprovalsEvent> for ApprovalsScreen {}

impl Render for ApprovalsScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let t = *theme(cx);
        let approvals = Session::get(cx).approvals.list.clone();
        let rows: Vec<_> = approvals.iter().map(|a| row(a, &t, cx)).collect();
        let empty = rows.is_empty().then(|| {
            div()
                .px(px(16.0))
                .py(px(14.0))
                .text_color(t.text_muted)
                .child("Nothing waiting")
        });
        let back = back_button("approvals.back")
            .on_click(cx.listener(|_, _, _, cx| cx.emit(ApprovalsEvent::Back)));
        div()
            .id("approvals")
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .child(top_bar(Some(back), "Approvals", &t))
            .children(empty)
            .children(rows)
    }
}

/// One approval: what the agent asks, and the buttons that answer it.
fn row(approval: &PendingApproval, t: &Theme, cx: &App) -> impl IntoElement + use<> {
    let chat = Session::get(cx)
        .chats
        .list
        .iter()
        .find(|c| c.id == approval.chat_id)
        .map_or_else(String::new, |c| c.title.clone());
    let allowed = approval.clone();
    let denied = approval.clone();
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(14.0))
        .border_b_1()
        .border_color(t.border)
        .child(
            div()
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .child(chat),
        )
        .child(div().text_color(t.text).child(approval.title.clone()))
        .child(
            div()
                .text_size(px(fs_sm()))
                .text_color(t.text_subtle)
                .child(approval.detail.clone()),
        )
        .child(
            div()
                .flex()
                .gap(px(12.0))
                .child(
                    Button::new(SharedString::from(format!(
                        "approvals.allow:{}",
                        approval.id
                    )))
                    .success()
                    .small()
                    .accessibility_label("Allow")
                    .on_click(move |_, _, cx| Session::answer(&allowed, true, cx)),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "approvals.deny:{}",
                        approval.id
                    )))
                    .small()
                    .accessibility_label("Deny")
                    .on_click(move |_, _, cx| Session::answer(&denied, false, cx)),
                ),
        )
}
