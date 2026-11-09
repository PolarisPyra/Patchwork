//! The panel's menus: shells, logs, and the log level.

use gpui::{ClickEvent, Context, Pixels, Point, Window};
use crate::ui::menu::{Dismissed, Menu, MenuItem};
use super::state::{MenuFor, Panel};

impl Panel {
    /// Opens menu `kind` at the pointer, or closes it when it is open. The press that closed it
    /// does not open it again.
    pub(super) fn toggle_menu(
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

    pub(super) fn open_menu(
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

    pub(super) fn menu_open(&self, kind: &MenuFor) -> bool {
        self.menu.as_ref().is_some_and(|(k, _, _)| k == kind)
    }

    pub(super) fn shells_menu(
        &mut self,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.menu_open(&MenuFor::Shells) {
            self.find_shells(cx);
        }
        let this = cx.entity().downgrade();
        let picked = self
            .shell_choices
            .iter()
            .find(|s| Some(&s.path) == self.picked.as_ref())
            .or(self.shell_choices.first())
            .map(|s| s.path.clone());
        let items = self
            .shell_choices
            .iter()
            .map(|s| {
                let (path, this) = (s.path.clone(), this.clone());
                MenuItem::new(s.name.clone(), move |_, cx| {
                    let path = path.clone();
                    let _ = this.update(cx, |p, cx| p.pick_shell(path, cx));
                })
                .radio(picked.as_ref() == Some(&s.path))
            })
            .collect();
        self.toggle_menu(MenuFor::Shells, "Shells", items, e, window, cx);
    }

    pub(super) fn logs_menu(
        &mut self,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let current = self.output.channel().map(|c| c.id.clone());
        let items = self
            .output
            .channels
            .iter()
            .map(|c| {
                let (id, this) = (c.id.clone(), this.clone());
                MenuItem::new(c.name.clone(), move |_, cx| {
                    let id = id.clone();
                    let _ = this.update(cx, |p, cx| p.pick_channel(id, cx));
                })
                .radio(current.as_ref() == Some(&c.id))
            })
            .collect();
        self.toggle_menu(MenuFor::Logs, "Logs", items, e, window, cx);
    }

    pub(super) fn level_menu(
        &mut self,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(channel) = self.output.channel().cloned() else {
            return;
        };
        let this = cx.entity().downgrade();
        let current = *(channel.setting)(&mut self.logging.clone());
        let items = crate::settings::LOG_LEVELS
            .iter()
            .map(|&(level, label)| {
                let this = this.clone();
                let setting = channel.setting;
                MenuItem::new(label, move |_, cx| {
                    let _ = this.update(cx, |p, cx| p.set_level(setting, level, cx));
                })
                .radio(level == current)
            })
            .collect();
        let label = format!("{} log level", channel.name);
        self.toggle_menu(MenuFor::Level, &label, items, e, window, cx);
    }
}
