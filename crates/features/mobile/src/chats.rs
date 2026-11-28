//! The list of a Patchwork's chats. A tap opens one.

// Rust guideline compliant 2026-11-27

use gpui::{
    Context, EventEmitter, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use patchwork_client::Session;
use patchwork_protocol::chats::ChatStatus;
use patchwork_design_components::{BtnKind, btn};
use patchwork_design_theme::{fs_sm, theme};

use crate::top_bar;

/// What the list asks the app for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatsEvent {
    /// Open this chat.
    Open(String),
    /// Show the approvals waiting.
    ShowApprovals,
}

/// The chat list, drawn from [`Session`].
#[derive(Debug)]
pub struct ChatsScreen;

impl ChatsScreen {
    /// Makes the list, which draws again whenever the session changes.
    #[must_use]
    pub fn new(cx: &mut Context<'_, Self>) -> Self {
        cx.observe_global::<Session>(|_, cx| cx.notify()).detach();
        Self
    }
}

impl EventEmitter<ChatsEvent> for ChatsScreen {}

impl Render for ChatsScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let t = *theme(cx);
        let waiting = Session::get(cx).approvals.list.len();
        let approvals = (waiting > 0).then(|| {
            btn(
                "chats.approvals",
                format!("{waiting} waiting"),
                BtnKind::Primary,
                false,
                &t,
            )
            .on_click(cx.listener(|_, _, _, cx| cx.emit(ChatsEvent::ShowApprovals)))
        });
        let session = Session::get(cx);
        let (hover, border) = (t.bg_hover, t.border);
        let rows = session
            .chats
            .list
            .iter()
            .filter(|c| c.archived_at.is_none())
            .map(|chat| {
                let id = chat.id.clone();
                let (status, color) = match chat.status {
                    ChatStatus::Running => ("Running", t.accent_text),
                    ChatStatus::Error => ("Failed", t.danger),
                    ChatStatus::Idle => ("", t.text_muted),
                };
                div()
                    .id(SharedString::from(format!("chats.chat:{}", chat.id)))
                    .flex()
                    .justify_between()
                    .gap(px(12.0))
                    .px(px(16.0))
                    .py(px(14.0))
                    .border_b_1()
                    .border_color(border)
                    .hover(move |s| s.bg(hover))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(t.text)
                            .child(chat.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(fs_sm()))
                            .text_color(color)
                            .child(status),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(ChatsEvent::Open(id.clone()))))
            });
        let head = top_bar(None, "Chats", &t)
            .justify_between()
            .children(approvals);
        div()
            .id("chats")
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .child(head)
            .children(rows)
    }
}
