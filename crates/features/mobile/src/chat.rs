//! One chat: its messages, and a box that sends to it or stops its turn.

// Rust guideline compliant 2026-11-27

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, px,
};
use patchwork_client::Session;
use patchwork_protocol::chats::ChatStatus;
use patchwork_protocol::messages::{Message, Part, Role};
use patchwork_design_components::{BtnKind, btn, input_box};
use patchwork_design_text::markdown::markdown;
use patchwork_design_theme::{Theme, fs_sm, theme};
use patchwork_views_chat::image_markdown::without_image_markdown;

use crate::{back_button, top_bar};

/// What the chat asks the app for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatEvent {
    /// Go back to the chat list.
    Back,
}

/// The open chat, drawn from [`Session`].
#[derive(Debug)]
pub struct ChatScreen {
    focus: FocusHandle,
    draft: String,
}

impl ChatScreen {
    /// Makes the screen, which draws again whenever the session changes.
    #[must_use]
    pub fn new(cx: &mut Context<'_, Self>) -> Self {
        cx.observe_global::<Session>(|_, cx| cx.notify()).detach();
        Self {
            focus: cx.focus_handle(),
            draft: String::new(),
        }
    }

    /// Takes a key typed into the box.
    fn key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<'_, Self>) {
        let keystroke = &event.keystroke;
        match keystroke.key.as_str() {
            "backspace" => {
                self.draft.pop();
            }
            "enter" => self.send(cx),
            _ => match &keystroke.key_char {
                Some(typed) if !keystroke.modifiers.control => self.draft.push_str(typed),
                _ => return,
            },
        }
        cx.notify();
    }

    /// Sends the draft, when there is one.
    fn send(&mut self, cx: &mut Context<'_, Self>) {
        let text = std::mem::take(&mut self.draft);
        if !text.trim().is_empty() {
            Session::send(text, cx);
        }
    }
}

impl EventEmitter<ChatEvent> for ChatScreen {}

impl Focusable for ChatScreen {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ChatScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let t = *theme(cx);
        let session = Session::get(cx);
        let title = session
            .chats
            .open_chat()
            .map_or_else(String::new, |chat| chat.title.clone());
        let running = session
            .chats
            .open_chat()
            .is_some_and(|chat| chat.status == ChatStatus::Running);
        let messages: Vec<_> = session
            .messages
            .list
            .iter()
            .map(|m| message(m, &t, cx))
            .collect();
        let back = back_button("chat.back", &t).on_click(cx.listener(|_, _, _, cx| {
            Session::close(cx);
            cx.emit(ChatEvent::Back);
        }));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(top_bar(Some(back), title, &t))
            .child(
                div()
                    .id("chat.messages")
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .px(px(16.0))
                    .py(px(12.0))
                    .overflow_y_scroll()
                    .children(messages),
            )
            .child(self.composer(running, &t, window, cx))
    }
}

impl ChatScreen {
    /// The box a reply is typed into, with its send or stop button.
    fn composer(
        &self,
        running: bool,
        t: &Theme,
        window: &Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let draft = if self.draft.is_empty() {
            div().text_color(t.text_muted).child("Message")
        } else {
            div().text_color(t.text).child(self.draft.clone())
        };
        let button = if running {
            btn("composer.stop", "Stop", BtnKind::Plain, false, t)
                .on_click(cx.listener(|_, _, _, cx| Session::stop(cx)))
        } else {
            btn("composer.send", "Send", BtnKind::Primary, false, t)
                .on_click(cx.listener(|this, _, _, cx| this.send(cx)))
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.0))
            .p(px(12.0))
            .border_t_1()
            .border_color(t.border)
            .child(
                input_box(self.focus.is_focused(window), t).flex_1().child(
                    div()
                        .id("composer.input")
                        .size_full()
                        .flex()
                        .items_center()
                        .track_focus(&self.focus)
                        .on_key_down(cx.listener(Self::key))
                        .on_click(cx.listener(|this, _, window, cx| {
                            window.focus(&this.focus, cx);
                            window.request_virtual_keyboard();
                        }))
                        .child(draft),
                ),
            )
            .child(button)
    }
}

/// One message: its text as markdown, and a line for each tool it ran.
fn message(message: &Message, t: &Theme, cx: &App) -> impl IntoElement + use<> {
    let lines = message
        .parts
        .iter()
        .enumerate()
        .filter_map(|(i, part)| match part {
            Part::Text { text } => Some(
                markdown(
                    &without_image_markdown(text),
                    format!("message.text:{}:{i}", message.id),
                    cx,
                )
                .text_color(t.text),
            ),
            Part::Tool { summary, .. } => Some(
                div()
                    .text_size(px(fs_sm()))
                    .text_color(t.text_muted)
                    .child(summary.clone()),
            ),
            Part::Approval { title, .. } => Some(div().text_color(t.warning).child(title.clone())),
            Part::Error { text } => Some(div().text_color(t.danger).child(text.clone())),
            Part::Reasoning { .. } | Part::Plan { .. } => None,
        });
    let bubble = div()
        .id(SharedString::from(format!(
            "message.content:{}",
            message.id
        )))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(10.0))
        .children(lines);
    match message.role {
        Role::User => bubble.bg(t.bubble).ml(px(48.0)),
        Role::Assistant => bubble,
    }
}
