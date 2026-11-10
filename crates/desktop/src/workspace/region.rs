//! One part of the workspace drawn as its own view: the sidebar, the chat, the status bar.
//!
//! GPUI redraws a view that is notified and reuses the last frame of every other cached one.
//! A scroll notifies the view it happens in, so with the chat in its own region a scroll of the
//! transcript redraws the chat alone, and the sidebar keeps its frame. Every change to the
//! workspace notifies each region, since each draws from the workspace's state.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Entity, Subscription, WeakEntity, Window, div, prelude::*};

use super::Workspace;

type Draw = fn(&mut Workspace, &mut Window, &mut Context<Workspace>) -> AnyElement;

pub struct Region {
    workspace: WeakEntity<Workspace>,
    draw: Draw,
    _changes: Subscription,
}

impl Region {
    pub fn new(workspace: &Entity<Workspace>, draw: Draw, cx: &mut Context<Self>) -> Self {
        Self {
            workspace: workspace.downgrade(),
            draw,
            _changes: cx.observe(workspace, |_, _, cx| cx.notify()),
        }
    }
}

impl Render for Region {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let draw = self.draw;
        let body = self.workspace.update(cx, |w, cx| draw(w, window, cx)).ok();
        div().size_full().flex().children(body)
    }
}
