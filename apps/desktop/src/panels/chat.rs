//! The chat's panel: the chat over its background, the details beside it, and the dock under
//! them.

// Rust guideline compliant 2026-10-09

use gpui::{Div, Stateful, StyleRefinement, div, prelude::*};

use crate::window::Workspace;

impl Workspace {
    /// The open chat with its details beside it and the dock under them.
    pub(super) fn chat_panel(
        &self,
        details: Option<Stateful<Div>>,
        dock: Option<impl IntoElement>,
    ) -> Stateful<Div> {
        div()
            .id("panels.chat")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.chat_layers())
                    .children(details),
            )
            .children(dock)
    }

    /// The chat over its background, each its own cached view: a moving backdrop redraws only
    /// itself and reuses the chat's last frame.
    pub(crate) fn chat_layers(&self) -> Div {
        div()
            .relative()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(
                self.backdrop_region
                    .clone()
                    .cached(StyleRefinement::default().absolute().inset_0()),
            )
            .child(
                self.chat_view
                    .clone()
                    .cached(StyleRefinement::default().flex_1().min_w_0().min_h_0()),
            )
    }
}
