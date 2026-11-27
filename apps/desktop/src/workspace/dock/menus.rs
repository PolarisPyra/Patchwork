//! Opening and closing the dock's menus, which its tabs fill.

// Rust guideline compliant 2026-10-09

use gpui::{ClickEvent, Context, Pixels, Point, Window};
use patchwork_ui_components::menu::{Dismissed, Menu, MenuItem};
use super::open_tabs::{MenuFor, Dock};

impl Dock {
    /// Opens menu `kind` at the pointer, or closes it when it is open. The press that closed it
    /// does not open it again.
    pub(crate) fn toggle_menu(
        &mut self,
        kind: MenuFor,
        label: &str,
        items: Vec<MenuItem>,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let closing_press = matches!(e, ClickEvent::Mouse(click)
            if self.menu_dismissed.take() == Some(click.down.position));
        if closing_press || self.menu.as_ref().is_some_and(|(k, _, _)| *k == kind) {
            self.menu = None;
            return cx.notify();
        }
        self.open_menu(kind, label, items, e.position(), window, cx);
    }

    pub(crate) fn open_menu(
        &mut self,
        kind: MenuFor,
        label: &str,
        items: Vec<MenuItem>,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = Menu::open(label.to_string(), items, window, cx);
        cx.subscribe_in(&menu, window, |this, _, e: &Dismissed, window, cx| {
            this.menu_dismissed = e.0;
            this.menu = None;
            window.focus(&this.focus, cx);
            cx.notify();
        })
        .detach();
        self.menu = Some((kind, menu, at));
        cx.notify();
    }

    pub(crate) fn menu_open(&self, kind: &MenuFor) -> bool {
        self.menu.as_ref().is_some_and(|(k, _, _)| k == kind)
    }
}
