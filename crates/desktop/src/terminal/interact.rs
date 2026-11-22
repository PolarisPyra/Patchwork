//! Typing, the mouse, the clipboard and scrolling in the active shell.

// Rust guideline compliant 2026-10-09

use std::time::Instant;

use gpui::{
    App, ClipboardItem, KeyDownEvent, Keystroke, Modifiers, Pixels, Point, PromptLevel,
    ScrollEvent, Window, prelude::*, px,
};

use crate::workspace::dock::Dock;
use crate::terminal::input::{Mouse, key_bytes, mouse_bytes};
use patchwork_ui::text::selection::from_primary;

use super::select::selected_text;
use super::shell::Metrics;

impl Dock {
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
    pub(crate) fn report_mouse(
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
    pub(crate) fn alt_click(&mut self, position: Point<Pixels>, metrics: &Metrics) -> bool {
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

    pub(crate) fn copy_selection(&mut self, cx: &mut App) -> bool {
        let Some(text) = self.active_shell_mut().and_then(selected_text) else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    pub(crate) fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
            self.paste_text(&text, window, cx);
        }
    }

    /// Pastes `text` into the shell. A line copied with its line end would run as soon as it
    /// landed, so the end comes off; several lines a shell would run one by one ask first,
    /// unless the program there takes a paste whole.
    pub(crate) fn paste_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.replace("\r\n", "\n");
        let text = match text.strip_suffix('\n') {
            Some(line) if !line.contains('\n') => line.to_string(),
            _ => text,
        };
        let bracketed = self
            .active_shell()
            .is_some_and(|s| s.parser.screen().bracketed_paste());
        if bracketed || !text.contains('\n') {
            return self.send_paste(&text, bracketed);
        }
        let lines = text.lines().count();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Paste {lines} lines into the shell?"),
            None,
            &["Paste", "Paste as one line", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let text = match answer.await {
                Ok(0) => text,
                Ok(1) => text
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
                _ => return,
            };
            let _ = this.update(cx, |this, cx| {
                this.send_paste(&text, false);
                cx.notify();
            });
        })
        .detach();
    }

    fn send_paste(&mut self, text: &str, bracketed: bool) {
        let text = text.replace('\n', "\r");
        self.type_text(if bracketed {
            format!("\x1b[200~{text}\x1b[201~")
        } else {
            text
        });
    }

    pub(crate) fn on_terminal_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ks = &e.keystroke;
        let m = &ks.modifiers;
        // Ctrl+Shift+C and V copy and paste. Ctrl+C copies too while text is selected, then
        // drops the selection so the next Ctrl+C reaches the shell as an interrupt; Ctrl+V
        // pastes. On macOS, Cmd copies and pastes and Ctrl+C always interrupts.
        let (copy_paste, plain) = if cfg!(target_os = "macos") {
            (m.platform && !m.shift, false)
        } else {
            (m.control && m.shift, m.control && !m.shift && !m.alt)
        };
        if plain && ks.key == "c" && self.copy_selection(cx) {
            if let Some(shell) = self.active_shell_mut() {
                shell.selection = None;
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if copy_paste && ks.key == "c" {
            if self.copy_selection(cx) {
                cx.stop_propagation();
            }
            return;
        }
        if (copy_paste || plain) && ks.key == "v" {
            self.paste(window, cx);
            cx.stop_propagation();
            return;
        }
        // Escape lets go of the selection first; with none, it reaches the program.
        if ks.key == "escape"
            && !m.modified()
            && let Some(shell) = self.active_shell_mut()
            && shell.selection.take().is_some()
        {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let select_all = if cfg!(target_os = "macos") {
            m.platform && !m.shift
        } else {
            m.control && m.shift
        };
        if select_all && ks.key == "a" {
            self.select_all(cx);
            cx.stop_propagation();
            return;
        }
        if self.scroll_key(ks) {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Shift+Insert pastes the primary selection, as in xterm.
        if ks.key == "insert" && m.shift && !m.control && !m.alt && !m.platform {
            if let Some(text) = from_primary(cx) {
                self.paste_text(&text, window, cx);
            }
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

    /// Scrolls the view through the lines kept for a key that does so: Shift+Page Up and Down by
    /// a screen, Ctrl+Shift+Up and Down by a line, Ctrl+Home and End to either end. A full-screen
    /// program keeps no lines, so it gets these keys itself.
    fn scroll_key(&mut self, ks: &Keystroke) -> bool {
        let m = &ks.modifiers;
        let primary = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control
        };
        if m.alt {
            return false;
        }
        let Some(shell) = self.active_shell_mut() else {
            return false;
        };
        let screen = shell.parser.screen_mut();
        if screen.alternate_screen() {
            return false;
        }
        let rows = usize::from(screen.size().0);
        let at = screen.scrollback();
        let to = match (ks.key.as_str(), primary, m.shift) {
            ("pageup", false, true) => at.saturating_add(rows),
            ("pagedown", false, true) => at.saturating_sub(rows),
            ("up", true, true) => at.saturating_add(1),
            ("down", true, true) => at.saturating_sub(1),
            ("home", true, false) => usize::MAX,
            ("end", true, false) => 0,
            _ => return false,
        };
        // The screen stops at the oldest line it keeps.
        screen.set_scrollback(to);
        true
    }

    pub(crate) fn on_terminal_scroll(
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
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// The cell under `position`, as (row, column).
    pub(crate) fn cell_under(&self, position: Point<Pixels>, metrics: &Metrics) -> (u16, u16) {
        let (cols, rows) = self.grid.unwrap_or((1, 1));
        let x = f32::from(position.x - self.origin.x) / metrics.cell_w;
        let y = f32::from(position.y - self.origin.y) / metrics.line_h;
        (
            (y.max(0.) as u16).min(rows.saturating_sub(1)),
            (x.max(0.) as u16).min(cols.saturating_sub(1)),
        )
    }

    /// The edge between cells nearest `position`, as (row, column), where a selection starts or ends.
    pub(crate) fn cell_at(&self, position: Point<Pixels>, metrics: &Metrics) -> (u16, u16) {
        let (cols, rows) = self.grid.unwrap_or((1, 1));
        let x = f32::from(position.x - self.origin.x) / metrics.cell_w;
        let y = f32::from(position.y - self.origin.y) / metrics.line_h;
        (
            (y.max(0.) as u16).min(rows.saturating_sub(1)),
            (x.max(0.).round() as u16).min(cols),
        )
    }
}
