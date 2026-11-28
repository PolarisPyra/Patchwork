//! The dock's height: as the user left it, kept inside the window, and changed by its handle.

// Rust guideline compliant 2026-10-09

use gpui::{Window, prelude::*};
use patchwork_client::local::core::Core;
use patchwork_design_theme::{STATUSBAR_H, TOOLBAR_H};

use crate::open_tabs::Dock;

/// The shortest the panel may be.
pub const PANEL_MIN: f32 = 120.;
const PANEL_MAX_FRACTION: f32 = 0.8;
const PANEL_DEFAULT: f32 = 260.;
const PANEL_HEIGHT_KEY: &str = "patchwork-panel-height";
/// How far the panel's height moves for each arrow key on its handle.
pub const PANEL_KEY_STEP: f32 = 16.;
/// The handle's grab area, much taller than the 1px line drawn, as VS Code's sash is.
pub const SASH_HIT: f32 = 8.;

/// The panel's height as the user last left it.
pub fn kept_height(core: &Core) -> f32 {
    core.state(PANEL_HEIGHT_KEY)
        .and_then(|h| h.parse().ok())
        .unwrap_or(PANEL_DEFAULT)
}

impl Dock {
    /// The panel's height, kept inside its bounds for the window's size now, and the most it
    /// may take. Maximized, it takes the most.
    #[must_use]
    pub fn bounded_height(&self, window: &Window) -> (f32, f32) {
        let column = f32::from(window.viewport_size().height) - TOOLBAR_H - STATUSBAR_H;
        bounded(self.height, column, self.maximized)
    }

    /// Starts a drag of the panel's handle at `y`, from the panel's `height` then.
    pub fn start_handle_drag(&mut self, y: f32, height: f32) {
        self.resizing = Some((y, height));
    }

    /// Whether the panel's handle is being dragged.
    #[must_use]
    pub fn is_resizing(&self) -> bool {
        self.resizing.is_some()
    }

    /// Follows a drag of the panel's handle to `y`.
    pub fn drag_handle(&mut self, y: f32, window: &Window, cx: &mut Context<Self>) {
        let Some((start_y, start_height)) = self.resizing else {
            return;
        };
        let (_, max) = self.bounded_height(window);
        self.height = dragged(start_y, start_height, y, max);
        cx.notify();
    }

    /// Ends a drag of the handle, keeping the height it left. Returns whether one was under way.
    pub fn end_handle_drag(&mut self) -> bool {
        if self.resizing.take().is_none() {
            return false;
        }
        self.keep_height();
        true
    }

    fn keep_height(&self) {
        let height = self.height.round().to_string();
        if let Err(e) = self.core.set_state(PANEL_HEIGHT_KEY, &height) {
            log::warn!(error:% = e; "could not keep the panel's height: {{error}}");
        }
    }

    /// Puts the panel back to its usual height, as a double-click or Home on its handle does.
    pub fn reset_height(&mut self, cx: &mut Context<Self>) {
        self.height = PANEL_DEFAULT;
        self.keep_height();
        cx.notify();
    }

    /// Sets the height from the arrow keys on the handle, within `max`.
    pub fn step_height(&mut self, height: f32, step: f32, max: f32, cx: &mut Context<Self>) {
        self.height = (height + step).clamp(PANEL_MIN, max);
        self.keep_height();
        cx.notify();
    }
}

/// A panel `height` tall kept inside a `column` of chat, and the most it may take there.
fn bounded(height: f32, column: f32, maximized: bool) -> (f32, f32) {
    let max = (column * PANEL_MAX_FRACTION).max(PANEL_MIN);
    if maximized {
        return (max, max);
    }
    (height.clamp(PANEL_MIN, max), max)
}

/// The height a handle drag started at `start_y` with `start_height` gives at `y`.
///
/// The handle sits on the panel's top edge, so dragging up makes the panel taller.
fn dragged(start_y: f32, start_height: f32, y: f32, max: f32) -> f32 {
    (start_height + start_y - y).clamp(PANEL_MIN, max)
}
