//! The details beside the chat: the agent's details while they are shown, then the file panel,
//! the Android panel or a quilt.

// Rust guideline compliant 2026-10-09

use gpui::{Div, Stateful, Window, div, prelude::*};

use crate::window::Workspace;

impl Workspace {
    /// What shows beside the chat, or None when nothing does. `chats` is whether the chats page
    /// fills the window, and `beside_chat` whether a chat, not an agent's page, is open there.
    pub(super) fn details_panel(
        &mut self,
        chats: bool,
        beside_chat: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let agent_details = if beside_chat && chats {
            self.agent_details_view(window, cx)
        } else {
            None
        };
        let canvas = self.canvas_area(window, cx).filter(|_| beside_chat);
        if agent_details.is_none() && canvas.is_none() {
            return None;
        }
        Some(
            div()
                .id("panels.details")
                .flex()
                .flex_none()
                .min_h_0()
                .children(agent_details)
                .children(canvas),
        )
    }
}
