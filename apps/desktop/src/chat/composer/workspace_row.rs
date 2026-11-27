//! The row on the composer's outer frame, as the web's workspace row: the chat's project, which
//! opens a list to move to another, the worktree, and the machine the chat runs on.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Div, MouseButton, Window, div, prelude::*, px};
use patchwork_ui_theme::{Theme, fs, fs_xs};
use patchwork_ui_components::{button, tooltip};
use patchwork_ui_theme::icon;

use super::chips::TRIGGER_H;
use super::draft::{Composer, Picks};
use super::menus::Menu;

/// What the row shows, read from the workspace before it is built.
pub(super) struct WorkspaceState {
    /// The project's name and folder, or none for a chat in no project.
    pub project: Option<(String, String)>,
    /// The SSH host the chat runs on; none is this computer.
    pub host: Option<String>,
    /// The worktree picker and the worktree's status, which the web's row lacks.
    pub worktree_pick: AnyElement,
    pub worktree: Option<AnyElement>,
}

impl Composer {
    /// The workspace row.
    pub(super) fn workspace_row(
        &mut self,
        s: WorkspaceState,
        picks: &Picks,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let hover = t.bg_hover;
        let (name, tip) = match &s.project {
            Some((name, path)) => (name.clone(), path.clone()),
            None => (
                "No project".to_owned(),
                "This chat is not in a project".to_owned(),
            ),
        };
        let project = button("project", format!("Select project, current: {name}"))
            .h(px(TRIGGER_H))
            .min_w_0()
            .max_w(px(300.))
            .gap(px(6.))
            .px(px(6.))
            .rounded(px(8.))
            .text_size(px(fs(15.)))
            .text_color(t.text)
            .hover(move |s| s.bg(hover))
            .tooltip(tooltip(tip))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .child(div().min_w_0().flex_1().truncate().child(name))
            .child(icon("caret-down", 14.).text_color(t.text_muted));
        let project = self.chip_with_menu(Menu::Project, project, picks, false, t, window, cx);
        let machine_tip = match &s.host {
            Some(host) => format!("Machine: {host}"),
            None => "Machine: this computer".to_owned(),
        };
        let machine = button("machine", "Machine")
            .size(px(24.))
            .flex_none()
            .justify_center()
            .rounded(px(6.))
            .text_color(t.text_muted)
            .hover(move |s| s.bg(hover))
            .tooltip(tooltip(machine_tip))
            .child(icon("desktop", 15.))
            .on_click(cx.listener(|this, _, window, cx| {
                // Settings' Devices pane, where the machines are added and picked.
                let _ = this
                    .workspace
                    .update(cx, |w, cx| w.open_settings(Some("ssh"), window, cx));
            }));
        let small = |child: AnyElement| {
            div()
                .min_w_0()
                .text_size(px(fs_xs()))
                .line_height(px(16.))
                .child(child)
        };
        div()
            .h(px(40.))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(4.))
            .pl(px(4.))
            .pr(px(2.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(project)
                    .child(small(s.worktree_pick))
                    .children(s.worktree.map(small)),
            )
            .child(machine)
    }
}
