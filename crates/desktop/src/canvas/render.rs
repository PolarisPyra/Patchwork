//! The canvas drawn.

// Rust guideline compliant 2026-10-09

use gpui::{KeyDownEvent, Role, Window, div, prelude::*, px};
use patchwork_server::threads::MessageStatus;
use crate::ui::theme::{FS_SM, theme};
use super::state::{Canvas, CanvasTab};

impl Render for Canvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let message = self.live_message(cx);
        // The agent may have edited the file while it was streaming.
        let streaming = message
            .as_ref()
            .is_some_and(|m| m.status == MessageStatus::Streaming);
        if self.was_streaming && !streaming {
            self.load(cx);
        }
        self.was_streaming = streaming;
        let header = self.header(&t, cx);
        let tabs = self.tabs(message.as_ref(), &t, cx);
        let body = self.body(message.as_ref(), window, cx);
        let footer = self.footer(&t, cx);
        let scrolls = self.active_tab() != CanvasTab::Code;
        div()
            .id("canvas")
            .role(Role::Complementary)
            .aria_label("File viewer")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" && (this.draft.is_some() || this.editing.is_some()) {
                    this.cancel_note(window, cx);
                    cx.stop_propagation();
                }
            }))
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .bg(t.bg_editor)
            .border_l_1()
            .border_color(t.border)
            .child(header)
            .child(tabs)
            .child(
                div()
                    .id("canvas-body")
                    .role(Role::TabPanel)
                    .flex_1()
                    .min_h_0()
                    .when(scrolls, |d| d.overflow_scroll().track_scroll(&self.body))
                    .child(body),
            )
            .when_some(self.action_error.clone(), |d, e| {
                d.child(
                    div()
                        .id("canvas-action-error")
                        .role(Role::Alert)
                        .flex_none()
                        .py(px(8.))
                        .px(px(20.))
                        .border_t_1()
                        .border_color(t.border)
                        .text_color(t.danger)
                        .text_size(px(FS_SM))
                        .child(e),
                )
            })
            .children(footer)
    }
}
