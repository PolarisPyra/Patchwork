//! The composer's pickers: the buttons in its footer and workspace row, the menus they open, and
//! what a pick does.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::f32::consts::TAU;
use gpui::{
    AnyElement, ClickEvent, Div, Focusable, MouseButton, PathBuilder, Stateful, Window, canvas,
    div, point, prelude::*, px,
};
use patchwork_ui_theme::{Theme, fs};
use patchwork_ui_components::{above, button};
use patchwork_ui_theme::icon;
use super::menus::{Menu, Pick};
use super::draft::{Composer, LaunchProject, Picks, offered};

/// The height of a picker button in the footer, as the web composer's `h-8`.
pub(super) const TRIGGER_H: f32 = 32.;

impl Composer {
    /// A chip was clicked: opens its menu, or closes it if it is open. The press that just
    /// closed it from outside does not open it again.
    pub(super) fn toggle_menu(
        &mut self,
        menu: Menu,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let ClickEvent::Mouse(click) = event
            && self.dismissed.take() == Some((menu, click.down.position))
        {
            return;
        }
        if self.menu == Some(menu) {
            return self.close_menu(window, cx);
        }
        self.open_menu(menu, window, cx);
    }

    /// Opens a chip's menu, as clicking it does.
    pub(super) fn open_menu(&mut self, menu: Menu, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_none() {
            self.menu_came_from = window.focused(cx);
        }
        self.menu = Some(menu);
        self.launching = false;
        self.picker = None;
        self.highlight = usize::MAX;
        match menu {
            Menu::Model => {
                if let Some(workspace) = self.workspace.upgrade() {
                    let w = workspace.read(cx);
                    let picks = self.picks(w);
                    let agents = offered(w, picks.agent);
                    self.open_model_picker(&agents, (picks.agent, picks.model), window, cx);
                }
            }
            Menu::Agent => {
                self.load_roles(cx);
                window.focus(&self.menu_focus, cx);
            }
            _ => window.focus(&self.menu_focus, cx),
        }
        cx.notify();
    }

    pub(super) fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.picker = None;
        self.effort_drag = None;
        self.restore_focus(window, cx);
        cx.notify();
    }

    /// Gives the keys back to where they were before a menu opened, so a menu closed by a press
    /// outside it does not leave them on a menu that is gone.
    pub(super) fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Back to where the keys were: picking a model is not a wish to write a message.
        let back = self.menu_came_from.take().or_else(|| {
            let workspace = self.workspace.upgrade()?;
            Some(workspace.read(cx).focus.clone())
        });
        if let Some(back) = back {
            window.focus(&back, cx);
        }
    }

    /// Applies a menu's pick and closes the menu.
    pub(super) fn apply(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        match pick {
            Pick::SendMode(queue) => {
                self.queue_next = Some(queue);
                self.close_menu(window, cx);
            }
            Pick::Attach(folder) => {
                self.close_menu(window, cx);
                self.attach(folder, cx);
            }
            Pick::Format(before, after) => {
                self.close_menu(window, cx);
                self.format(before, after, window, cx);
            }
            Pick::Insert(text) => {
                self.close_menu(window, cx);
                window.focus(&self.editor.focus_handle(cx), cx);
                self.editor.update(cx, |e, cx| {
                    let at = e.caret().unwrap_or(e.content.len());
                    e.replace(at..at, text, window, cx);
                });
            }
            pick => {
                self.close_menu(window, cx);
                self.commit(pick, cx);
            }
        }
    }

    /// Puts Markdown marks around the selection, or `before` at the start of the line when
    /// there is nothing to put after it, and gives the keys back to the editor.
    pub(super) fn format(
        &mut self,
        before: &'static str,
        after: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.editor.focus_handle(cx), cx);
        self.editor.update(cx, |e, cx| {
            if after.is_empty() {
                e.prefix_line(before, window, cx);
            } else {
                e.surround(before, after, window, cx);
            }
        });
    }

    /// Changes what a pick changes, with the menu left as it is: the model picker stays open
    /// while a model and its effort are tried in turn.
    ///
    /// The Orchestrator page's picks are kept for the chats its request starts; otherwise they go
    /// to the open chat, or to the draft of the next one.
    pub(super) fn commit(&mut self, pick: Pick, cx: &mut Context<Self>) {
        if self.launching {
            match pick {
                Pick::Model(agent, model) => self.launch = Some((agent, model)),
                Pick::Effort(effort) => self.draft.effort = effort,
                Pick::Project(project) => self.launch_project = LaunchProject::Picked(project),
                _ => {}
            }
            return cx.notify();
        }
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        if let Pick::Project(project) = pick {
            // As picking it in the sidebar does: its newest chat opens, or a new one in it.
            workspace.update(cx, |w, cx| {
                if let Some(id) = project {
                    w.select_project(&id, cx);
                } else {
                    w.remember_project(None);
                    w.new_chat(cx);
                }
            });
            return;
        }
        let chat = workspace.read(cx).chat.as_ref().map(|t| t.id.clone());
        let Some(id) = chat else {
            match pick {
                Pick::Model(agent, model) => {
                    self.draft.agent = Some(agent);
                    self.draft.model = model;
                }
                Pick::Effort(effort) => self.draft.effort = effort,
                Pick::Compact(window) => self.draft.compact = window,
                Pick::Access(mode) => self.draft.access = mode,
                Pick::Role(role) => self.draft.role = role,
                Pick::CompactNow
                | Pick::SendMode(_)
                | Pick::Project(_)
                | Pick::Attach(_)
                | Pick::Format(..)
                | Pick::Insert(_) => {}
            }
            return cx.notify();
        };
        // New chats start with the access, effort and compact window last picked.
        match &pick {
            Pick::Effort(effort) => self.draft.effort.clone_from(effort),
            Pick::Compact(window) => self.draft.compact = *window,
            Pick::Access(mode) => self.draft.access = *mode,
            _ => {}
        }
        workspace.update(cx, |w, cx| {
            let core = Arc::clone(&w.core);
            let done = match pick {
                Pick::Model(agent, model) => core.chats().set_model(&id, agent, model),
                Pick::Effort(effort) => core.chats().set_effort(&id, effort.as_deref()),
                Pick::Compact(window) => core.set_chat_compact_window(&id, window),
                Pick::Access(mode) => core.chats().set_access_mode(&id, mode),
                Pick::Role(role) => core.set_chat_role(&id, role),
                Pick::CompactNow => {
                    w.send_command("/compact", cx);
                    Ok(())
                }
                Pick::SendMode(_)
                | Pick::Project(_)
                | Pick::Attach(_)
                | Pick::Format(..)
                | Pick::Insert(_) => Ok(()),
            };
            if let Err(e) = done {
                log::warn!(error:% = e; "could not change the chat: {{error}}");
            }
            w.reload(cx);
        });
    }

    /// A chip, with its menu open above it when it is the open one.
    #[expect(
        clippy::too_many_arguments,
        reason = "each argument is a separate piece of what it draws"
    )]
    pub(super) fn chip_with_menu(
        &mut self,
        menu: Menu,
        chip: Stateful<Div>,
        picks: &Picks,
        can_compact: bool,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        // The Orchestrator page's menus are drawn there.
        let open = self.menu == Some(menu) && !self.launching;
        let hover = t.bg_hover;
        let chip = chip
            .aria_expanded(open)
            .when(open, |chip| chip.bg(hover))
            .on_click(
                cx.listener(move |this, e, window, cx| this.toggle_menu(menu, e, window, cx)),
            );
        let body = open.then(|| {
            if menu == Menu::Model {
                let agents = self
                    .workspace
                    .upgrade()
                    .map(|w| offered(w.read(cx), picks.agent))
                    .unwrap_or_default();
                return self.model_picker(
                    &agents,
                    (picks.agent, picks.model.as_deref(), picks.effort.as_deref()),
                    t,
                    window,
                    cx,
                );
            }
            let rows = if menu == Menu::Project {
                self.workspace
                    .upgrade()
                    .map(|w| {
                        let w = w.read(cx);
                        let current = w.project().map(|p| p.id.clone());
                        self.project_rows(&w.settings, current.as_deref(), t)
                    })
                    .unwrap_or_default()
            } else {
                self.rows(menu, picks, can_compact, t)
            };
            self.menu_picks = rows.iter().map(|r| r.pick.clone()).collect();
            if self.highlight >= rows.len() {
                self.highlight = rows.iter().position(|r| r.checked).unwrap_or(0);
            }
            let focus = self.menu_focus.clone();
            self.menu_box(menu, rows, &focus, t, cx)
        });
        div()
            .relative()
            .flex()
            .min_w_0()
            .child(chip)
            .when_some(body, |d, body| d.child(above(body)))
    }
}

/// A picker's button in the footer: its icon, the value chosen and a caret, quiet until hovered,
/// as the web composer's `PickerTrigger`. A press on it leaves the keys where they are.
pub(super) fn chip(
    id: &'static str,
    aria: String,
    lead: AnyElement,
    label: String,
    t: &Theme,
) -> Stateful<Div> {
    trigger(id, aria, t)
        .child(lead)
        .child(div().min_w_0().max_w(px(180.)).truncate().child(label))
        .child(icon("caret-down", 12.).text_color(t.text_muted))
}

/// The bare button a picker opens from, for a caller to fill.
pub(super) fn trigger(id: &'static str, aria: String, t: &Theme) -> Stateful<Div> {
    let hover = t.bg_hover;
    button(id, aria)
        .h(px(TRIGGER_H))
        .flex_none()
        .min_w_0()
        .gap(px(6.))
        .pl(px(10.))
        .pr(px(8.))
        .rounded(px(10.))
        .text_size(px(fs(14.)))
        .text_color(t.text)
        .hover(move |s| s.bg(hover))
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
}

/// How full the context is against where it compacts, as a ring that turns the warning colour
/// once compacting is near.
pub(super) fn fill_ring(fill: f32, t: &Theme) -> AnyElement {
    let color = if fill >= 0.85 {
        t.warning
    } else {
        t.text_muted
    };
    canvas(
        |_, _, _| {},
        move |bounds, (), window: &mut Window, _| {
            let c = bounds.center();
            let r = 5.5;
            let at = |turn: f32| {
                let a = (turn - 0.25) * TAU;
                point(c.x + px(r * a.cos()), c.y + px(r * a.sin()))
            };
            let radii = point(px(r), px(r));
            let mut ring = PathBuilder::stroke(px(2.));
            ring.move_to(at(0.));
            ring.arc_to(radii, px(0.), false, true, at(0.5));
            ring.arc_to(radii, px(0.), false, true, at(0.));
            if let Ok(path) = ring.build() {
                window.paint_path(path, color.opacity(0.25));
            }
            let fill = fill.min(1.);
            if fill > 0. {
                let mut arc = PathBuilder::stroke(px(2.));
                arc.move_to(at(0.));
                arc.arc_to(radii, px(0.), fill > 0.5, true, at(fill.min(0.9999)));
                if let Ok(path) = arc.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .size(px(14.))
    .flex_none()
    .into_any_element()
}
