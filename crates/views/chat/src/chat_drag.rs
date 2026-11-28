//! A chat being dragged, from a pane's tabs or the sidebar's task tree, and what follows the
//! pointer while it is.

// Rust guideline compliant 2026-10-09

use gpui::{Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, div, px};
use patchwork_design_theme::{fs_sm, radius, theme};

/// How wide a chat's tab is, which the dragged chat's ghost matches.
pub const TAB_WIDTH: f32 = 180.;

/// A chat being dragged: its id, and its title for what follows the pointer.
#[derive(Clone, Debug)]
pub struct ChatDrag {
    /// The dragged chat's id.
    pub chat: String,
    /// The dragged chat's title.
    pub title: SharedString,
}

/// What follows the pointer while a chat is dragged: its title in a tab's shape.
#[derive(Debug)]
pub struct ChatGhost(pub SharedString);

impl Render for ChatGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        div()
            .w(px(TAB_WIDTH))
            .h(px(28.))
            .flex()
            .items_center()
            .px(px(10.))
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_panel)
            .text_size(px(fs_sm()))
            .text_color(t.text)
            .shadow_md()
            .child(div().min_w_0().truncate().child(self.0.clone()))
    }
}
