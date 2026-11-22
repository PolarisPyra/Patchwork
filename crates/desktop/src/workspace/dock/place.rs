//! The dock under the chat with the handle that resizes it, and the keys that open, close and
//! switch it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{App, Entity, MouseButton, Role, Window, div, prelude::*, px};
use patchwork_core::Core;
use patchwork_core::settings::Settings;
use patchwork_core::system::ssh::is_remote;

use crate::workspace::Workspace;
use super::open_tabs::{Dock, DockEvent, DockTab};
use patchwork_ui::theme::{STATUSBAR_H, TOOLBAR_H};

const PANEL_MIN: f32 = 120.;
const PANEL_MAX_FRACTION: f32 = 0.8;
const PANEL_DEFAULT: f32 = 260.;
const PANEL_HEIGHT_KEY: &str = "patchwork-panel-height";

/// The panel, closed, and its height as the user left it.
pub(crate) fn new_panel(
    core: Arc<Core>,
    settings: &Settings,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> (Entity<Dock>, f32) {
    let height = core
        .state(PANEL_HEIGHT_KEY)
        .and_then(|h| h.parse().ok())
        .unwrap_or(PANEL_DEFAULT);
    let panel = cx.new(|cx| Dock::new(core, settings, cx));
    cx.subscribe_in(
        &panel,
        window,
        |this, _, e: &DockEvent, window, cx| match e {
            DockEvent::Close => this.close_panel(window, cx),
            DockEvent::SettingsChanged => {
                this.settings = this.core.settings();
                cx.notify();
            }
            DockEvent::RunFinished { code, output } => {
                if let Some(android) = &this.android {
                    android.update(cx, |a, cx| a.run_finished(*code, output, cx));
                }
            }
        },
    )
    .detach();
    (panel, height)
}

impl Workspace {
    /// Hands the panel the settings and the folder new terminals start in: the open task's
    /// worktree, or else the open project's folder.
    pub(crate) fn sync_panel(&mut self, cx: &mut Context<Self>) {
        let host = self.machine_host(cx);
        let cwd = self
            .project()
            .filter(|p| p.host() == host.as_deref())
            .map(|p| self.local_chat_folder().unwrap_or_else(|| p.path.clone()));
        self.dock
            .update(cx, |p, cx| p.sync(&self.settings, host, cwd, cx));
    }

    fn open_panel(&mut self, tab: DockTab, cx: &mut Context<Self>) {
        self.sync_panel(cx);
        self.panel_open = true;
        self.dock.update(cx, |p, cx| p.show(tab, cx));
        cx.notify();
    }

    /// Runs the command `launch` starts in a new terminal tab, without opening the panel.
    pub fn run_in_terminal(
        &mut self,
        launch: patchwork_core::terminal::Launch,
        cx: &mut Context<Self>,
    ) {
        self.sync_panel(cx);
        self.dock.update(cx, |p, cx| p.run(launch, cx));
        cx.notify();
    }

    /// Types `text` into the terminal of the command [`Self::run_in_terminal`] started.
    pub fn type_in_run(&self, text: &str, cx: &App) {
        self.dock.read(cx).type_in_task(text);
    }

    pub(crate) fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.panel_open {
            return;
        }
        let had_focus = self.dock.read(cx).contains_focus(window, cx);
        self.panel_open = false;
        self.dock.update(cx, Dock::hide);
        if had_focus {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// The terminal key opens the terminal, focuses it if the panel shows something else, and
    /// closes the panel from inside it. In a task with its own worktree it opens on a shell in
    /// that worktree, as the worktree menu's "Open terminal here" does.
    pub(crate) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.dock.read(cx);
        if self.panel_open && panel.tab == DockTab::Terminal && panel.terminal_focused(window) {
            return self.close_panel(window, cx);
        }
        self.open_panel(DockTab::Terminal, cx);
        let folder = self.local_chat_folder();
        self.dock.update(cx, |p, cx| {
            if let Some(folder) = folder {
                p.switch_to_shell_in(&folder, cx);
            }
            p.focus_terminal(window, cx);
        });
    }

    /// The open task's worktree when it is on this computer.
    fn local_chat_folder(&self) -> Option<String> {
        self.chat_folder().filter(|f| !is_remote(f))
    }

    pub(crate) fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_panel(cx);
        self.panel_open = true;
        self.dock.update(cx, |p, cx| {
            p.new_terminal(cx);
            p.focus_terminal(window, cx);
        });
        cx.notify();
    }

    /// Opens a new terminal in the folder `cwd` on the project's machine.
    pub fn new_terminal_in(&mut self, cwd: String, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_panel(cx);
        self.panel_open = true;
        self.dock.update(cx, |p, cx| {
            p.new_terminal_in(cwd, cx);
            p.focus_terminal(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn toggle_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel_open {
            self.close_panel(window, cx);
        } else {
            let tab = self.dock.read(cx).tab;
            self.open_panel(tab, cx);
        }
    }

    pub(crate) fn toggle_output(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel_open && self.dock.read(cx).tab == DockTab::Output {
            self.close_panel(window, cx);
        } else {
            self.open_panel(DockTab::Output, cx);
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
    pub(crate) fn drag_panel(&mut self, y: f32, window: &Window, cx: &mut Context<Self>) {
        let Some((start_y, start_height)) = self.panel_resizing else {
            return;
        };
        let (_, max) = self.panel_height(window);
        self.panel_height = (start_height + start_y - y).clamp(PANEL_MIN, max);
        cx.notify();
    }

    pub(crate) fn keep_panel_height(&self) {
        let height = self.panel_height.round().to_string();
        if let Err(e) = self.core.set_state(PANEL_HEIGHT_KEY, &height) {
            log::warn!(error:% = e; "could not keep the panel's height: {{error}}");
        }
    }

    /// The handle and the panel, while it is open.
    pub(crate) fn panel_area(
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
            .aria_numeric_value(f64::from(height.round()))
            .aria_min_numeric_value(f64::from(PANEL_MIN))
            .aria_max_numeric_value(f64::from(max.round()))
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
                .child(div().h(px(height)).min_h_0().child(self.dock.clone())),
        )
    }
}
