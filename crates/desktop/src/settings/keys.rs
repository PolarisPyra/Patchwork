//! Moving through Settings with the keyboard.

// Rust guideline compliant 2026-10-09

use gpui::{App, Context, KeyDownEvent, Window};
use super::dialog::{Section, SettingsDialog, SettingsEvent, sections};

impl SettingsDialog {
    pub(super) fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.record_key(e, cx) || self.record_sigil(e, cx) {
            return cx.stop_propagation();
        }
        let step = match e.keystroke.key.as_str() {
            "escape" => {
                cx.stop_propagation();
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
        // Up and Down move along the list while a section of it has focus.
        let all: Vec<&'static Section> = sections().collect();
        let Some(at) = all.iter().position(|s| s.id == self.section) else {
            return;
        };
        if !self.nav_focused(window, cx) {
            return;
        }
        let next = (at as isize + step).clamp(0, all.len() as isize - 1) as usize;
        self.open_section(all[next].id, cx);
        // The focus goes along when a section's button has it, as the buttons are next to each other.
        let on_list = self
            .general
            .nav_focus
            .as_ref()
            .is_some_and(|f| f.is_focused(window));
        if next != at && !on_list {
            if step > 0 {
                window.focus_next(cx);
            } else {
                window.focus_prev(cx);
            }
        }
        cx.stop_propagation();
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
