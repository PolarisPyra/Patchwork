//! Patchwork's phone screens: pairing with a Patchwork, the chat list, one chat, and the approvals
//! waiting. Each is a view drawn from [`Session`](patchwork_client::Session) in the shared theme
//! and components, and asks the app to move between them with an event.

// Rust guideline compliant 2026-11-27

pub mod approvals;
pub mod chat;
pub mod chats;
pub mod pairing;

pub use approvals::{ApprovalsEvent, ApprovalsScreen};
pub use chat::{ChatEvent, ChatScreen};
pub use chats::{ChatsEvent, ChatsScreen};
pub use pairing::{PairingEvent, PairingScreen};

use gpui::component::button::Button;
use gpui::component::Sizable as _;
use gpui::{Div, ElementId, IntoElement, ParentElement as _, Styled as _, div, px};
use patchwork_design_theme::{Theme, fs_xl};

/// A screen's top bar: a Back button when `back` is given, then the title.
fn top_bar(back: Option<Button>, title: impl IntoElement, t: &Theme) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.0))
        .px(px(16.0))
        .py(px(12.0))
        .border_b_1()
        .border_color(t.border)
        .children(back)
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(fs_xl()))
                .text_color(t.text)
                .child(title),
        )
}

/// A Back button with `id`.
fn back_button(id: impl Into<ElementId>) -> Button {
    Button::new(id).small().label("Back")
}
