//! `panels.dock`: the dock under the chat with the handle that resizes it, and the keys that
//! open, close and switch it.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_chat::prelude::*;
use patchwork_client::AppState;
use std::sync::Arc;

use gpui::{App, Entity, MouseButton, Role, Window, div, prelude::*, px};
use patchwork_core::Core;
use patchwork_core::settings::Settings;
use patchwork_platform::ssh::is_remote;

use crate::window::Workspace;
use patchwork_desktop_dock::{Dock, DockEvent, DockTab, PANEL_KEY_STEP, PANEL_MIN, SASH_HIT};
use patchwork_design_theme::theme;

/// The panel, closed, at the height the user left it.
pub(crate) fn new_panel(
    core: Arc<Core>,
    settings: &Settings,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Entity<Dock> {
    let panel = cx.new(|cx| Dock::new(core, settings, cx));
    cx.subscribe_in(
        &panel,
        window,
        |this, _, e: &DockEvent, window, cx| match e {
            DockEvent::Close => this.close_panel(window, cx),
            DockEvent::Maximized => cx.notify(),
            DockEvent::SettingsChanged => {
                AppState::update(cx, AppState::reload_settings);
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
    panel
}

impl Workspace {
    /// Hands the panel the settings and the folder new terminals start in: the open task's
    /// worktree, or else the open project's folder.
    pub(crate) fn sync_panel(&mut self, cx: &mut Context<Self>) {
        let host = self.machine_host(cx);
        let cwd = self
            .project(cx)
            .filter(|p| p.host() == host.as_deref())
            .map(|p| self.local_chat_folder().unwrap_or_else(|| p.path.clone()));
        let settings = AppState::get(cx).settings.shared();
        self.dock
            .update(cx, |p, cx| p.sync(&settings, host, cwd, cx));
    }

    pub(crate) fn open_panel(&mut self, tab: DockTab, cx: &mut Context<Self>) {
        self.sync_panel(cx);
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

    /// Runs the command `launch` starts in a new terminal tab called `name`, and shows it.
    pub(crate) fn show_in_terminal(
        &mut self,
        launch: patchwork_core::terminal::Launch,
        name: String,
        cx: &mut Context<Self>,
    ) {
        self.sync_panel(cx);
        self.dock.update(cx, |p, cx| {
            p.open_terminal_tab(cx);
            p.run_named(launch, Some(name), cx);
        });
        cx.notify();
    }

    /// Types `text` into the terminal of the command [`Self::run_in_terminal`] started.
    pub fn type_in_run(&self, text: &str, cx: &App) {
        self.dock.read(cx).type_in_task(text);
    }

    pub(crate) fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.panel_open(cx) {
            return;
        }
        let had_focus = self.dock.read(cx).contains_focus(window, cx);
        self.dock.update(cx, Dock::hide);
        if had_focus {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// The terminal key opens the terminal, focuses it if the panel shows something else, and
    /// closes the panel from inside it. It opens on a shell in the open task's worktree or the
    /// open project's folder, starting one there when none exists.
    pub(crate) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.dock.read(cx);
        if panel.visible && panel.tab == DockTab::Terminal && panel.terminal_focused(window) {
            return self.close_panel(window, cx);
        }
        self.open_panel(DockTab::Terminal, cx);
        self.dock.update(cx, |p, cx| p.focus_terminal(window, cx));
    }

    /// The open task's worktree when it is on this computer.
    fn local_chat_folder(&self) -> Option<String> {
        self.chat_folder().filter(|f| !is_remote(f))
    }

    pub(crate) fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_panel(cx);
        self.dock.update(cx, |p, cx| {
            p.new_terminal(cx);
            p.focus_terminal(window, cx);
        });
        cx.notify();
    }

    /// Opens a new terminal in the folder `cwd` on the project's machine.
    pub fn new_terminal_in(&mut self, cwd: String, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_panel(cx);
        self.dock.update(cx, |p, cx| {
            p.new_terminal_in(cwd, cx);
            p.focus_terminal(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn toggle_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel_open(cx) {
            self.close_panel(window, cx);
        } else {
            let tab = self.dock.read(cx).tab;
            self.open_panel(tab, cx);
        }
    }

    pub(crate) fn toggle_output(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dock = self.dock.read(cx);
        if dock.visible && dock.tab == DockTab::Output {
            self.close_panel(window, cx);
        } else {
            self.open_panel(DockTab::Output, cx);
        }
    }

    /// Whether the panel under the chat shows.
    pub(crate) fn panel_open(&self, cx: &App) -> bool {
        self.dock.read(cx).visible
    }

    /// The panel with the handle on its top edge that resizes it, while it is open. The handle
    /// is a 1px line that lights up in the accent colour while hovered or dragged; maximized,
    /// the panel has none.
    pub(crate) fn panel_area(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let dock = self.dock.read(cx);
        if !dock.visible {
            return None;
        }
        let (height, max) = dock.bounded_height(window);
        let maximized = dock.maximized;
        let dragging = dock.is_resizing();
        let t = *theme(cx);
        let accent = t.accent;
        let handle = div()
            .id("panel-resize")
            .group("panel-resize")
            .role(Role::Splitter)
            .aria_label("Resize panel")
            .aria_orientation(gpui::Orientation::Horizontal)
            .aria_numeric_value(f64::from(height.round()))
            .aria_min_numeric_value(f64::from(PANEL_MIN))
            .aria_max_numeric_value(f64::from(max.round()))
            .tab_index(0)
            .absolute()
            .left_0()
            .right_0()
            .top(px(-SASH_HIT / 2.))
            .h(px(SASH_HIT + 1.))
            .cursor_row_resize()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(SASH_HIT / 2.))
                    .h(px(1.))
                    .bg(if dragging { accent } else { t.border })
                    .group_hover("panel-resize", move |s| s.bg(accent)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                    this.dock.update(cx, |d, _| {
                        d.start_handle_drag(f32::from(e.position.y), height);
                    });
                    cx.notify();
                }),
            )
            .on_click(cx.listener(|this, e: &gpui::ClickEvent, _, cx| {
                if e.click_count() == 2 {
                    this.dock.update(cx, Dock::reset_height);
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, _, cx| {
                let step = match e.keystroke.key.as_str() {
                    "up" => PANEL_KEY_STEP,
                    "down" => -PANEL_KEY_STEP,
                    "home" => {
                        cx.stop_propagation();
                        this.dock.update(cx, Dock::reset_height);
                        return cx.notify();
                    }
                    _ => return,
                };
                this.dock
                    .update(cx, |d, cx| d.step_height(height, step, max, cx));
                cx.stop_propagation();
                cx.notify();
            }));
        Some(
            div()
                .id("panels.dock")
                .relative()
                .flex_none()
                .h(px(height))
                .min_h_0()
                .child(self.dock.clone())
                .when(!maximized, |area| area.child(handle)),
        )
    }
}
