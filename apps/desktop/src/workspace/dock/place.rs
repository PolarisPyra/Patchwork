//! The dock under the chat with the handle that resizes it, and the keys that open, close and
//! switch it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{App, Entity, MouseButton, Role, Window, div, prelude::*, px};
use patchwork_backend::Core;
use patchwork_backend::settings::Settings;
use patchwork_platform::ssh::is_remote;

use crate::workspace::Workspace;
use super::open_tabs::{Dock, DockEvent, DockTab};
use patchwork_ui_theme::{STATUSBAR_H, TOOLBAR_H, theme};

const PANEL_MIN: f32 = 120.;
const PANEL_MAX_FRACTION: f32 = 0.8;
const PANEL_DEFAULT: f32 = 260.;
const PANEL_HEIGHT_KEY: &str = "patchwork-panel-height";
/// How far the panel's height moves for each arrow key on its handle.
const PANEL_KEY_STEP: f32 = 16.;
/// The handle's grab area, much taller than the 1px line drawn, as VS Code's sash is.
const SASH_HIT: f32 = 8.;

/// The panel's height as the user last left it.
pub(crate) fn kept_height(core: &Core) -> f32 {
    core.state(PANEL_HEIGHT_KEY)
        .and_then(|h| h.parse().ok())
        .unwrap_or(PANEL_DEFAULT)
}

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
    panel
}

impl Dock {
    /// The panel's height, kept inside its bounds for the window's size now, and the most it
    /// may take. Maximized, it takes the most.
    fn bounded_height(&self, window: &Window) -> (f32, f32) {
        let column = f32::from(window.viewport_size().height) - TOOLBAR_H - STATUSBAR_H;
        bounded(self.height, column, self.maximized)
    }

    /// Follows a drag of the panel's handle to `y`.
    pub(crate) fn drag_handle(&mut self, y: f32, window: &Window, cx: &mut Context<Self>) {
        let Some((start_y, start_height)) = self.resizing else {
            return;
        };
        let (_, max) = self.bounded_height(window);
        self.height = dragged(start_y, start_height, y, max);
        cx.notify();
    }

    /// Ends a drag of the handle, keeping the height it left. Returns whether one was under way.
    pub(crate) fn end_handle_drag(&mut self) -> bool {
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
    fn reset_height(&mut self, cx: &mut Context<Self>) {
        self.height = PANEL_DEFAULT;
        self.keep_height();
        cx.notify();
    }

    /// Sets the height from the arrow keys on the handle, within `max`.
    fn step_height(&mut self, height: f32, step: f32, max: f32, cx: &mut Context<Self>) {
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
        self.dock.update(cx, |p, cx| p.show(tab, cx));
        cx.notify();
    }

    /// Runs the command `launch` starts in a new terminal tab, without opening the panel.
    pub fn run_in_terminal(
        &mut self,
        launch: patchwork_backend::terminal::Launch,
        cx: &mut Context<Self>,
    ) {
        self.sync_panel(cx);
        self.dock.update(cx, |p, cx| p.run(launch, cx));
        cx.notify();
    }

    /// Runs the command `launch` starts in a new terminal tab called `name`, and shows it.
    pub(crate) fn show_in_terminal(
        &mut self,
        launch: patchwork_backend::terminal::Launch,
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
    /// Whether the panel is open on the terminal.
    pub(crate) fn terminal_shown(&self, cx: &App) -> bool {
        let dock = self.dock.read(cx);
        dock.visible && dock.tab == DockTab::Terminal
    }

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
        let dragging = dock.resizing.is_some();
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
                        d.resizing = Some((f32::from(e.position.y), height));
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
                .relative()
                .flex_none()
                .h(px(height))
                .min_h_0()
                .child(self.dock.clone())
                .when(!maximized, |area| area.child(handle)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{PANEL_MIN, bounded, dragged};

    #[test]
    fn height_stays_within_the_column() {
        assert_eq!(bounded(260., 1000., false), (260., 800.));
        assert_eq!(bounded(950., 1000., false), (800., 800.));
        assert_eq!(bounded(10., 1000., false), (PANEL_MIN, 800.));
    }

    #[test]
    fn a_short_column_still_allows_the_least_height() {
        assert_eq!(bounded(260., 100., false), (PANEL_MIN, PANEL_MIN));
    }

    #[test]
    fn maximized_takes_the_most() {
        assert_eq!(bounded(200., 1000., true), (800., 800.));
    }

    #[test]
    fn dragging_up_grows_the_panel_within_bounds() {
        assert_eq!(dragged(500., 260., 400., 800.), 360.);
        assert_eq!(dragged(500., 260., 600., 800.), 160.);
        assert_eq!(dragged(500., 260., 900., 800.), PANEL_MIN);
        assert_eq!(dragged(500., 260., -400., 800.), 800.);
    }
}
