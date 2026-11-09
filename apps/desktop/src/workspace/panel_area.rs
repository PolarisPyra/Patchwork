//! The panel under the chat with the handle that resizes it, and the keys that open, close and
//! switch it (`PanelArea.tsx`).

use std::sync::Arc;

use gpui::{Context, Entity, IntoElement, MouseButton, Role, Window, div, prelude::*, px};
use patchwork_server::Server;
use patchwork_server::settings::Settings;

use super::Workspace;
use crate::panel::{Panel, PanelEvent, PanelTab};
use crate::ui::theme::{STATUSBAR_H, TOOLBAR_H};

const PANEL_MIN: f32 = 120.;
const PANEL_MAX_FRACTION: f32 = 0.8;
const PANEL_DEFAULT: f32 = 260.;
const PANEL_HEIGHT_KEY: &str = "patchwork-panel-height";

/// The panel, closed, and its height as the user left it.
pub(super) fn new_panel(
    server: Arc<Server>,
    settings: &Settings,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> (Entity<Panel>, f32) {
    let height = server
        .store
        .state(PANEL_HEIGHT_KEY)
        .and_then(|h| h.parse().ok())
        .unwrap_or(PANEL_DEFAULT);
    let panel = cx.new(|cx| Panel::new(server, settings, cx));
    cx.subscribe_in(
        &panel,
        window,
        |this, _, e: &PanelEvent, window, cx| match e {
            PanelEvent::Close => this.close_panel(window, cx),
            PanelEvent::SettingsChanged => {
                this.settings = this.server.store.settings();
                cx.notify();
            }
        },
    )
    .detach();
    (panel, height)
}

impl Workspace {
    /// Hands the panel the settings and the open project's folder.
    pub(super) fn sync_panel(&mut self, cx: &mut Context<Self>) {
        let cwd = self.project().map(|p| p.path.clone());
        let settings = self.settings.clone();
        self.panel.update(cx, |p, cx| p.sync(&settings, cwd, cx));
    }

    fn open_panel(&mut self, tab: PanelTab, cx: &mut Context<Self>) {
        self.sync_panel(cx);
        self.panel_open = true;
        self.panel.update(cx, |p, cx| p.show(tab, cx));
        cx.notify();
    }

    pub(super) fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.panel_open {
            return;
        }
        let had_focus = self.panel.read(cx).contains_focus(window, cx);
        self.panel_open = false;
        self.panel.update(cx, |p, cx| p.hide(cx));
        if had_focus {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// The terminal key opens the terminal, focuses it if the panel shows something else, and
    /// closes the panel from inside it.
    pub(super) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.panel.read(cx);
        if self.panel_open && panel.tab == PanelTab::Terminal && panel.terminal_focused(window) {
            return self.close_panel(window, cx);
        }
        self.open_panel(PanelTab::Terminal, cx);
        self.panel.update(cx, |p, cx| p.focus_terminal(window, cx));
    }

    pub(super) fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_panel(cx);
        self.panel_open = true;
        self.panel.update(cx, |p, cx| {
            p.new_terminal(cx);
            p.focus_terminal(window, cx);
        });
        cx.notify();
    }

    pub(super) fn toggle_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel_open {
            self.close_panel(window, cx);
        } else {
            let tab = self.panel.read(cx).tab;
            self.open_panel(tab, cx);
        }
    }

    pub(super) fn toggle_output(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel_open && self.panel.read(cx).tab == PanelTab::Output {
            self.close_panel(window, cx);
        } else {
            self.open_panel(PanelTab::Output, cx);
        }
    }

    /// The panel's height, kept inside its bounds for the window's size now, and the most it
    /// may take.
    fn panel_height(&self, window: &Window) -> (f32, f32) {
        let column = f32::from(window.viewport_size().height) - TOOLBAR_H - STATUSBAR_H;
        let max = (column * PANEL_MAX_FRACTION).max(PANEL_MIN);
        (self.panel_height.clamp(PANEL_MIN, max), max)
    }

    /// Follows a drag of the panel's handle to `y`.
    pub(super) fn drag_panel(&mut self, y: f32, window: &Window, cx: &mut Context<Self>) {
        let Some((start_y, start_height)) = self.panel_resizing else {
            return;
        };
        let (_, max) = self.panel_height(window);
        self.panel_height = (start_height + start_y - y).clamp(PANEL_MIN, max);
        cx.notify();
    }

    pub(super) fn keep_panel_height(&self) {
        let height = self.panel_height.round().to_string();
        if let Err(e) = self.server.store.set_state(PANEL_HEIGHT_KEY, &height) {
            log::warn!("could not keep the panel's height: {e}");
        }
    }

    /// The handle and the panel, while it is open.
    pub(super) fn panel_area(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        if !self.panel_open {
            return None;
        }
        let (height, max) = self.panel_height(window);
        let handle = div()
            .id("panel-resize")
            .role(Role::Splitter)
            .aria_label("Resize panel")
            .aria_orientation(gpui::Orientation::Horizontal)
            .aria_numeric_value(height.round() as f64)
            .aria_min_numeric_value(PANEL_MIN as f64)
            .aria_max_numeric_value(max.round() as f64)
            .tab_index(0)
            .h(px(4.))
            .mb(px(-4.))
            .flex_none()
            .cursor_row_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                    this.panel_resizing = Some((f32::from(e.position.y), height));
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, _, cx| {
                let step = match e.keystroke.key.as_str() {
                    "up" => 16.,
                    "down" => -16.,
                    _ => return,
                };
                this.panel_height = (height + step).clamp(PANEL_MIN, max);
                this.keep_panel_height();
                cx.stop_propagation();
                cx.notify();
            }));
        Some(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .child(handle)
                .child(div().h(px(height)).min_h_0().child(self.panel.clone())),
        )
    }
}
