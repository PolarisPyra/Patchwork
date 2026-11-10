//! Typing, the mouse, the clipboard and scrolling in the active shell.

// Rust guideline compliant 2026-10-09

use std::time::Instant;

use gpui::{
    App, ClipboardItem, KeyDownEvent, Modifiers, Pixels, Point, ScrollEvent, Window, prelude::*, px,
};

use crate::panel::Panel;
use crate::panel::cells::ordered;
use crate::panel::input::{Mouse, key_bytes, mouse_bytes};

use super::shell::{Metrics, Shell};

impl Panel {
    fn type_text(&mut self, data: String) {
        if let Some(shell) = self.active_shell_mut()
            && shell.exit_code.is_none()
        {
            // Typing brings the view back to the prompt.
            shell.parser.screen_mut().set_scrollback(0);
            shell.selection = None;
            let _ = shell.input.unbounded_send(data);
            self.blink_from = Instant::now();
        }
    }

    /// Tells the program on the screen what the mouse did, when it asked to hear of it and Shift,
    /// which keeps the mouse for selecting, is up. Returns whether it was told.
    pub(super) fn report_mouse(
        &mut self,
        event: Mouse,
        position: Point<Pixels>,
        m: Modifiers,
        metrics: &Metrics,
    ) -> bool {
        if m.shift {
            return false;
        }
        let cell = self.cell_under(position, metrics);
        let moved = matches!(event, Mouse::Drag(_) | Mouse::Move);
        let Some(shell) = self.active_shell() else {
            return false;
        };
        let Some(bytes) = mouse_bytes(shell.parser.screen(), event, cell, m) else {
            return false;
        };
        // A move within the cell last reported says nothing new.
        if !(moved && self.mouse_cell == Some(cell)) {
            let _ = shell.input.unbounded_send(bytes);
        }
        self.mouse_cell = Some(cell);
        true
    }

    /// `altClickMovesCursor`: Alt+click on the prompt sends the arrow keys that walk the cursor
    /// there, counting cells across wrapped lines. Returns whether it did.
    pub(super) fn alt_click(&mut self, position: Point<Pixels>, metrics: &Metrics) -> bool {
        let (row, col) = self.cell_under(position, metrics);
        let Some(shell) = self.active_shell() else {
            return false;
        };
        let screen = shell.parser.screen();
        if screen.alternate_screen() || screen.scrollback() > 0 {
            return false;
        }
        let (cur_row, cur_col) = screen.cursor_position();
        let cols = i32::from(screen.size().1);
        let cells =
            (i32::from(row) - i32::from(cur_row)) * cols + i32::from(col) - i32::from(cur_col);
        let key = match (cells > 0, screen.application_cursor()) {
            (true, true) => "\x1bOC",
            (true, false) => "\x1b[C",
            (false, true) => "\x1bOD",
            (false, false) => "\x1b[D",
        };
        self.type_text(key.repeat(cells.unsigned_abs() as usize));
        true
    }

    pub(super) fn copy_selection(&self, cx: &mut App) -> bool {
        let Some(text) = self.active_shell().and_then(selected_text) else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    pub(super) fn paste(&mut self, cx: &mut App) {
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else {
            return;
        };
        let bracketed = self
            .active_shell()
            .is_some_and(|s| s.parser.screen().bracketed_paste());
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        self.type_text(if bracketed {
            format!("\x1b[200~{text}\x1b[201~")
        } else {
            text
        });
    }

    pub(in crate::panel) fn on_terminal_key(
        &mut self,
        e: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ks = &e.keystroke;
        let m = &ks.modifiers;
        // Ctrl+Shift+C and V copy and paste, as Ctrl+C and V belong to the shell.
        let copy_paste = if cfg!(target_os = "macos") {
            m.platform && !m.shift
        } else {
            m.control && m.shift
        };
        if copy_paste && ks.key == "c" {
            if self.copy_selection(cx) {
                cx.stop_propagation();
            }
            return;
        }
        if copy_paste && ks.key == "v" {
            self.paste(cx);
            cx.stop_propagation();
            return;
        }
        let app_cursor = self
            .active_shell()
            .is_some_and(|s| s.parser.screen().application_cursor());
        let Some(bytes) = key_bytes(ks, app_cursor) else {
            return;
        };
        self.type_text(bytes);
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn on_terminal_scroll(
        &mut self,
        e: &ScrollEvent,
        metrics: &Metrics,
        cx: &mut Context<Self>,
    ) {
        let speed = if e.modifiers.alt {
            self.settings.fast_scroll_sensitivity
        } else {
            self.settings.mouse_wheel_scroll_sensitivity
        };
        let line_h = px(metrics.line_h);
        let Some(shell) = self.active_shell_mut() else {
            return;
        };
        let lines =
            f32::from(e.delta.pixel_delta(line_h).y) / metrics.line_h * speed + shell.scroll_rest;
        let whole = lines.trunc();
        shell.scroll_rest = lines - whole;
        if whole == 0. {
            return;
        }
        let wheel = if whole > 0. {
            Mouse::WheelUp
        } else {
            Mouse::WheelDown
        };
        // A program that hears the mouse gets one wheel report a turn, however far it went, as in xterm.
        if self.report_mouse(wheel, e.position, e.modifiers, metrics) {
            cx.stop_propagation();
            return;
        }
        let Some(shell) = self.active_shell_mut() else {
            return;
        };
        let screen = shell.parser.screen();
        if screen.alternate_screen() {
            // Full-screen programs scroll with the arrow keys.
            let key = match (whole > 0., screen.application_cursor()) {
                (true, true) => "\x1bOA",
                (true, false) => "\x1b[A",
                (false, true) => "\x1bOB",
                (false, false) => "\x1b[B",
            };
            let _ = shell.input.unbounded_send(key.repeat(whole.abs() as usize));
        } else {
            let back = shell.parser.screen().scrollback() as f32 + whole;
            shell
                .parser
                .screen_mut()
                .set_scrollback(back.max(0.) as usize);
            shell.selection = None;
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// The cell under `position`, as (row, column).
    pub(super) fn cell_under(&self, position: Point<Pixels>, metrics: &Metrics) -> (u16, u16) {
        let (cols, rows) = self.grid.unwrap_or((1, 1));
        let x = f32::from(position.x - self.origin.x) / metrics.cell_w;
        let y = f32::from(position.y - self.origin.y) / metrics.line_h;
        (
            (y.max(0.) as u16).min(rows.saturating_sub(1)),
            (x.max(0.) as u16).min(cols.saturating_sub(1)),
        )
    }

    /// The edge between cells nearest `position`, as (row, column), where a selection starts or ends.
    pub(super) fn cell_at(&self, position: Point<Pixels>, metrics: &Metrics) -> (u16, u16) {
        let (cols, rows) = self.grid.unwrap_or((1, 1));
        let x = f32::from(position.x - self.origin.x) / metrics.cell_w;
        let y = f32::from(position.y - self.origin.y) / metrics.line_h;
        (
            (y.max(0.) as u16).min(rows.saturating_sub(1)),
            (x.max(0.).round() as u16).min(cols),
        )
    }
}

fn selected_text(shell: &Shell) -> Option<String> {
    let (a, b) = ordered(shell.selection?);
    if a == b {
        return None;
    }
    let text = shell.parser.screen().contents_between(a.0, a.1, b.0, b.1);
    (!text.is_empty()).then_some(text)
}
