//! The chips under the draft and the menus they open.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::f32::consts::TAU;
use gpui::{
    AnyElement, ClickEvent, Div, FontWeight, MouseButton, PathBuilder, Stateful, Window, canvas,
    div, point, prelude::*, px,
};
use patchwork_server::threads::Thread;
use crate::ui::theme::{fs_md, radius, radius_sm, Theme};
use crate::ui::{above, button, icon};
use super::menus::{Menu, Pick};
use super::state::{Composer, Picks, offered};

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

    /// Applies a menu's pick to the open chat, or to the draft of the next one.
    pub(super) fn apply(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        if let Pick::SendMode(queue) = pick {
            self.queue_next = Some(queue);
            self.close_menu(window, cx);
            return;
        }
        let launching = self.launching;
        self.close_menu(window, cx);
        if launching {
            match pick {
                Pick::Model(agent, model) => self.launch = Some((agent, model)),
                Pick::Effort(effort) => self.draft.effort = effort,
                _ => {}
            }
            return;
        }
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let thread = workspace.read(cx).thread.as_ref().map(|t| t.id.clone());
        let Some(id) = thread else {
            match pick {
                Pick::Model(agent, model) => {
                    self.draft.agent = Some(agent);
                    self.draft.model = model;
                }
                Pick::Effort(effort) => self.draft.effort = effort,
                Pick::Compact(window) => self.draft.compact = window,
                Pick::Access(mode) => self.draft.access = mode,
                Pick::Role(role) => self.draft.role = role,
                Pick::CompactNow | Pick::SendMode(_) => {}
            }
            return;
        };
        // New chats start with the access, effort and compact window last picked.
        match &pick {
            Pick::Effort(effort) => self.draft.effort = effort.clone(),
            Pick::Compact(window) => self.draft.compact = *window,
            Pick::Access(mode) => self.draft.access = *mode,
            _ => {}
        }
        workspace.update(cx, |w, cx| {
            let server = Arc::clone(&w.server);
            let done = match pick {
                Pick::Model(agent, model) => server.set_thread_model(&id, agent, model),
                Pick::Effort(effort) => server.set_thread_effort(&id, effort.as_deref()),
                Pick::Compact(window) => server.set_thread_compact_window(&id, window),
                Pick::Access(mode) => server.set_thread_access_mode(&id, mode),
                Pick::Role(role) => server.set_thread_role(&id, role),
                Pick::CompactNow => {
                    w.send_command("/compact", cx);
                    Ok(())
                }
                Pick::SendMode(_) => Ok(()),
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
        let chip = chip.aria_expanded(open).on_click(
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
                    (picks.agent, picks.model.as_deref()),
                    t,
                    window,
                    cx,
                );
            }
            let rows = self.rows(menu, picks, can_compact, t);
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
            .child(chip)
            .when_some(body, |d, body| d.child(above(body)))
    }
}

/// `CHIP_CLASS`: a picker's button, its value muted until hovered.
pub(super) fn chip(
    id: &'static str,
    aria: String,
    lead: AnyElement,
    label: String,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(id, aria)
        .h(px(28.))
        .gap(px(4.))
        .rounded(px(radius_sm()))
        .px(px(6.))
        .text_size(px(fs_md()))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        // A press on a chip opens its menu and leaves the keys where they are; the composer
        // around it would otherwise take them for its editor.
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .child(lead)
        .child(div().max_w(px(180.)).truncate().child(label))
        .child(icon("caret-down", 14.))
}

/// A 32px square tool button: formatting and attaching.
pub(super) fn tool_button(
    id: &'static str,
    label: &'static str,
    name: &'static str,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(id, label)
        .size(px(32.))
        .flex_none()
        .justify_center()
        .rounded(px(radius()))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon(name, 16.))
}

/// How full the context is against where it compacts, as a ring that turns the warning colour
/// once compacting is near.
pub(super) fn fill_ring(fill: f32, t: &Theme) -> AnyElement {
    let color = if fill >= 0.9 { t.warning } else { t.text_muted };
    canvas(
        |_, _, _| {},
        move |bounds, (), window: &mut Window, _| {
            let c = bounds.center();
            let r = 6.;
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
                window.paint_path(path, color.opacity(0.3));
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
    .size(px(16.))
    .flex_none()
    .into_any_element()
}

/// The chat's context now, from the last reply that reported it.
pub(super) fn context_of(thread: &Thread) -> Option<u64> {
    thread
        .messages
        .iter()
        .rev()
        .find_map(|m| m.usage.as_ref().map(|u| u.context))
}
