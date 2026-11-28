//! The Usage popover made for the window, and the keys it hands back when it closes.

// Rust guideline compliant 2026-10-09

use gpui::{Entity, Window, prelude::*};
use patchwork_desktop_usage::{Usage, UsageEvent};

use crate::window::Workspace;

impl Workspace {
    /// Makes the sidebar's Usage button and popover, which starts reading usage at once.
    pub(crate) fn new_usage(window: &mut Window, cx: &mut Context<Self>) -> Entity<Usage> {
        let usage = cx.new(Usage::new);
        cx.subscribe_in(&usage, window, |this, _, event, window, cx| match event {
            UsageEvent::Closed => window.focus(&this.focus, cx),
        })
        .detach();
        usage
    }

    /// Closes the Usage popover without taking the keys, for when they are going somewhere else.
    pub fn dismiss_usage(&mut self, cx: &mut Context<Self>) {
        self.usage
            .update(cx, patchwork_desktop_usage::Usage::dismiss);
    }
}
