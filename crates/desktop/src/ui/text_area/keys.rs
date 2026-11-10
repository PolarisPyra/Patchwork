//! What the keys and the mouse do to the text field: moving, selecting, editing, pasting.

// Rust guideline compliant 2026-10-09

use std::ops::Range;

use gpui::{
    ClipboardItem, EntityInputHandler, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    Window, point, prelude::*, px,
};
use super::state::{
    Arrow, Backspace, BackspaceEmpty, Copy, CtrlSubmit, Cut, Delete, Down, End, Home, Left,
    Newline, Paste, Right, SelectAll, SelectDown, SelectLeft, SelectRight, SelectUp, Submit,
    Submitted, SubmittedBack, Tab, Tabbed, TextArea, Up,
};

impl TextArea {
    /// The offset a row above (`rows` < 0) or below the cursor.
    fn vertical(&self, rows: f32) -> Option<usize> {
        let at = self.position_for(self.cursor())?;
        let y = at.y + self.line_height * rows + self.line_height / 2.;
        if y < px(0.) {
            return Some(0);
        }
        let last = self.lines.last()?;
        let bottom = last.top + last.layout.size(self.line_height).height;
        if y > bottom {
            return Some(self.content.len());
        }
        Some(self.offset_for(point(at.x, y)))
    }

    pub(super) fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        let to = if self.selected.is_empty() {
            self.previous_boundary(self.cursor())
        } else {
            self.selected.start
        };
        self.move_to(to, window, cx);
    }

    pub(super) fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        let to = if self.selected.is_empty() {
            self.next_boundary(self.selected.end)
        } else {
            self.selected.end
        };
        self.move_to(to, window, cx);
    }

    pub(super) fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        if self.single_line || self.menu_keys {
            return cx.emit(Arrow(-1));
        }
        let to = self.vertical(-1.).unwrap_or(0);
        self.move_to(to, window, cx);
    }

    pub(super) fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        if self.single_line || self.menu_keys {
            return cx.emit(Arrow(1));
        }
        let to = self.vertical(1.).unwrap_or(self.content.len());
        self.move_to(to, window, cx);
    }

    pub(super) fn select_left(
        &mut self,
        _: &SelectLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.previous_boundary(self.cursor()), window, cx);
    }

    pub(super) fn select_right(
        &mut self,
        _: &SelectRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.next_boundary(self.cursor()), window, cx);
    }

    pub(super) fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        let to = self.vertical(-1.).unwrap_or(0);
        self.select_to(to, window, cx);
    }

    pub(super) fn select_down(
        &mut self,
        _: &SelectDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let to = self.vertical(1.).unwrap_or(self.content.len());
        self.select_to(to, window, cx);
    }

    pub(super) fn select_all(
        &mut self,
        _: &SelectAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_everything(window, cx);
    }

    /// Selects the whole text, so typing replaces it.
    pub fn select_everything(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
        self.select_to(self.content.len(), window, cx);
    }

    /// The start of the cursor's line.
    pub(super) fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        let start = self.content[..self.cursor()]
            .rfind('\n')
            .map_or(0, |i| i + 1);
        self.move_to(start, window, cx);
    }

    /// The end of the cursor's line.
    pub(super) fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.cursor();
        let end = self.content[cursor..]
            .find('\n')
            .map_or(self.content.len(), |i| cursor + i);
        self.move_to(end, window, cx);
    }

    pub(super) fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.content.is_empty() {
            return cx.emit(BackspaceEmpty);
        }
        if self.selected.is_empty() {
            self.select_to(self.previous_boundary(self.cursor()), window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(super) fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.select_to(self.next_boundary(self.cursor()), window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(super) fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.single_line {
            return cx.emit(SubmittedBack);
        }
        self.replace_text_in_range(None, "\n", window, cx);
    }

    pub(super) fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        if self.enter_newline {
            return self.replace_text_in_range(None, "\n", window, cx);
        }
        cx.emit(Submitted);
    }

    pub(super) fn ctrl_submit(&mut self, _: &CtrlSubmit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(Submitted);
    }

    /// Tab picks from an open menu; otherwise it moves the focus on.
    pub(super) fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        if self.menu_keys {
            cx.emit(Tabbed);
        } else {
            cx.propagate();
        }
    }

    pub(super) fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = text.replace("\r\n", "\n");
            // A one-line field has no line to break, and a token copied with its line end is
            // not the token.
            let text = if self.single_line {
                text.trim_matches('\n').replace('\n', " ")
            } else {
                text
            };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    pub(super) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected.clone()].to_string(),
            ));
        }
    }

    pub(super) fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    pub(super) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.offset_at(event.position);
        // A second press selects the word under it, and a third its line.
        if event.click_count > 1 {
            self.selecting = false;
            self.selected = if event.click_count == 2 {
                self.word_at(offset)
            } else {
                self.line_at(offset)
            };
            self.reversed = false;
            window.notify_text_selection_changed(&self.focus);
            return cx.notify();
        }
        self.selecting = true;
        if event.modifiers.shift {
            self.select_to(offset, window, cx);
        } else {
            self.move_to(offset, window, cx);
        }
    }

    pub(super) fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    pub(super) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selecting {
            let offset = self.offset_at(event.position);
            self.select_to(offset, window, cx);
        }
    }

    /// The offset under a point in the window.
    fn offset_at(&self, position: Point<Pixels>) -> usize {
        match self.bounds {
            Some(bounds) => self.offset_for(position - bounds.origin),
            None => self.content.len(),
        }
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    pub(super) fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        let mut utf8 = 0;
        for ch in self.content.chars() {
            if utf8 >= offset {
                break;
            }
            utf8 += ch.len_utf8();
            utf16 += ch.len_utf16();
        }
        utf16
    }

    pub(super) fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub(super) fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}
