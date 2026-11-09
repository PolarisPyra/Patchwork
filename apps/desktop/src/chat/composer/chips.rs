//! The chips under the draft and the menus they open.

use std::f32::consts::TAU;
use gpui::{
    AnyElement, ClickEvent, Context, Div, Focusable, FontWeight, IntoElement, PathBuilder,
    Stateful, Window, canvas, div, point, prelude::*, px,
};
use patchwork_server::threads::Thread;
use crate::ui::theme::{FS_MD, RADIUS, RADIUS_SM, Theme};
use crate::ui::{above, button, icon};
use super::menus::{Menu, Pick};
use super::state::{Composer, Picks};

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
        self.menu = Some(menu);
        self.picker = None;
        self.highlight = usize::MAX;
        match menu {
            Menu::Model => {
                if let Some(workspace) = self.workspace.upgrade() {
                    let w = workspace.read(cx);
                    let picks = self.picks(w);
                    let agents = Self::offered(w, picks.agent);
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
        let focus = self.editor.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Applies a menu's pick to the open chat, or to the draft of the next one.
    pub(super) fn apply(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        if let Pick::SendMode(queue) = pick {
            if !queue || self.can_queue {
                self.queue_next = Some(queue);
                self.close_menu(window, cx);
            }
            return;
        }
        self.close_menu(window, cx);
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
            let server = w.server.clone();
            let done = match pick {
                Pick::Model(agent, model) => server.set_thread_model(&id, agent, model),
                Pick::Effort(effort) => server.set_thread_effort(&id, effort),
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
                log::warn!("could not change the chat: {e}");
            }
            w.reload(cx);
        });
    }

    /// A chip, with its menu open above it when it is the open one.
    #[allow(clippy::too_many_arguments)]
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
        let open = self.menu == Some(menu);
        let chip = chip.aria_expanded(open).on_click(
            cx.listener(move |this, e, window, cx| this.toggle_menu(menu, e, window, cx)),
        );
        let body = open.then(|| {
            if menu == Menu::Model {
                let agents = self
                    .workspace
                    .upgrade()
                    .map(|w| Self::offered(w.read(cx), picks.agent))
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
        .rounded(px(RADIUS_SM))
        .px(px(6.))
        .text_size(px(FS_MD))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
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
        .rounded(px(RADIUS))
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
        move |bounds, _, window: &mut Window, _| {
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
