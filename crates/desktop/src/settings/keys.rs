//! Moving through Settings with the keyboard.

// Rust guideline compliant 2026-10-09

use gpui::{App, Context, Focusable, KeyDownEvent, Window};
use super::dialog::{SettingsDialog, SettingsEvent};
use super::nav::pane_order;

impl SettingsDialog {
    pub(super) fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.record_key(e, cx) || self.record_sigil(e, cx) {
            return cx.stop_propagation();
        }
        let step = match e.keystroke.key.as_str() {
            "escape" => {
                cx.stop_propagation();
                // The search empties first.
                if !self.query(cx).is_empty() {
                    return self.clear_search(cx);
                }
                // A form open in the section closes first, and the dialog on the next press.
                let closed = self.cancel_draft(cx)
                    || match self.section {
                        "mcp" => self.cancel_mcp_form(cx),
                        "ssh" => self.cancel_add_device(cx),
                        "scheduled" => self.cancel_task(cx),
                        "providers" => {
                            self.cancel_prompt(cx)
                                || self.cancel_address(cx)
                                || self.cancel_form(cx)
                        }
                        _ => false,
                    };
                if closed {
                    return;
                }
                return cx.emit(SettingsEvent::Closed);
            }
            "up" => -1,
            "down" => 1,
            _ => return,
        };
        if !self.nav_focused(window, cx) {
            return;
        }
        cx.stop_propagation();
        // On a search result, Up and Down move the highlight from the search field.
        if !self.query(cx).is_empty() {
            if let Some(search) = &self.search {
                let focus = search.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
            return self.step_hit(step, cx);
        }
        // Up and Down walk every pane in the list's order, and the focus goes along.
        let panes: Vec<_> = pane_order().collect();
        let Some(at) = panes.iter().position(|id| *id == self.section) else {
            return;
        };
        let next = panes[(at as isize + step).clamp(0, panes.len() as isize - 1) as usize];
        if next != self.section {
            self.open_section(next, cx);
        }
        self.focus_row(next, window, cx);
    }

    /// Tab and Shift+Tab go round the dialog's own controls, never on to the window behind it.
    pub(super) fn cycle_focus(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Past its last control the window's order goes on behind it, so step until back inside.
        for _ in 0..1000 {
            if forward {
                window.focus_next(cx);
            } else {
                window.focus_prev(cx);
            }
            if self.focus.contains_focused(window, cx) {
                break;
            }
        }
    }

    pub(super) fn nav_focused(&self, window: &Window, cx: &App) -> bool {
        self.general
            .nav_focus
            .as_ref()
            .is_some_and(|f| f.contains_focused(window, cx))
    }
}
