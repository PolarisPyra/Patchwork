//! The glass pill a message is written in, shared by the chat's composer and the Orchestrator
//! page: its frame, its round send button, and the chips under it.

// Rust guideline compliant 2026-10-09

use gpui::{
    BoxShadow, Div, ElementId, FontWeight, Hsla, MouseButton, SharedString, Stateful, div, point,
    prelude::*, px,
};

use super::inputs::button;
use crate::theme::fs_sm;

/// The send button's size.
pub const SEND: f32 = 36.;

/// The space between the send button and the pill's edge.
pub const PILL_PAD: f32 = 8.;

/// How round the pill's corners are: half its height while it holds one line, so a one-line pill
/// is round at the ends and a taller one keeps the same corners.
pub const PILL_RADIUS: f32 = SEND / 2. + PILL_PAD;

/// The pill's frame: its corners, its border, its glass and the shadow under it.
pub fn pill(id: impl Into<ElementId>, border: Hsla, glass: Hsla, shadow: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .rounded(px(PILL_RADIUS))
        .border_1()
        .border_color(border)
        .bg(glass)
        .shadow(vec![BoxShadow {
            color: shadow,
            offset: point(px(0.), px(6.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        }])
}

/// The round button at the pill's end, in the text's color with the glass's color on it, or
/// faint while there is nothing to send. Its corners are the caller's, as a split button rounds
/// only one side.
pub fn send_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    ready: bool,
    ink: Hsla,
    paper: Hsla,
) -> Stateful<Div> {
    let base = button(id, label)
        .size(px(SEND))
        .flex_none()
        .justify_center();
    if ready {
        let hover = ink.opacity(0.8);
        base.bg(ink.opacity(0.92))
            .text_color(paper)
            .hover(move |s| s.bg(hover))
    } else {
        base.bg(ink.opacity(0.14))
            .text_color(ink.opacity(0.5))
            .cursor_default()
    }
}

/// A chip under the pill that opens a menu, its text faint until hovered. A press on it leaves
/// the keys in the field, where they go back once a pick is made.
pub fn pill_chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    ink: Hsla,
) -> Stateful<Div> {
    let shade = ink.opacity(0.08);
    button(id, label)
        .h(px(28.))
        .gap(px(6.))
        .px(px(10.))
        .rounded_full()
        .text_size(px(fs_sm()))
        .font_weight(FontWeight::MEDIUM)
        .text_color(ink.opacity(0.7))
        .hover(move |s| s.bg(shade).text_color(ink))
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
}
