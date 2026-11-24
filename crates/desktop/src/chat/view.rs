//! The chat's view: the state of one chat on screen, apart from the workspace around it.
//!
//! The pane system shows several chats at once, each in a view of its own. The chat's state moves
//! here from the workspace a part at a time; until it all has, the view draws through the
//! workspace, handing itself to the drawing for the parts that live here.

// Rust guideline compliant 2026-10-09

use gpui::{Entity, FocusHandle, Subscription, WeakEntity, Window, div, prelude::*};

use crate::chat::top_right::find_bar::FindState;
use crate::workspace::Workspace;

/// One chat on screen and the state of its view.
pub struct ChatView {
    pub(crate) workspace: WeakEntity<Workspace>,
    /// Where the keys go back to when nothing in the chat had them.
    pub(crate) home_focus: FocusHandle,
    pub(crate) find: FindState,
    _changes: Subscription,
}

impl ChatView {
    /// A view drawing from `workspace`, redrawn whenever it changes.
    pub fn new(
        workspace: &Entity<Workspace>,
        home_focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            workspace: workspace.downgrade(),
            home_focus,
            find: FindState::default(),
            _changes: cx.observe(workspace, |_, _, cx| cx.notify()),
        }
    }
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.clone();
        let body = workspace
            .update(cx, |w, cx| crate::chat::panes::panes(w, self, window, cx))
            .ok();
        div().size_full().flex().children(body)
    }
}
