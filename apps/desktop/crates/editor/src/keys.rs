//! What the keys do to the text field: moving, selecting, editing, pasting.

// Rust guideline compliant 2026-10-09

use std::ops::Range;

use gpui::{ClipboardItem, EntityInputHandler, Window, point, prelude::*, px};
use super::field::{
    Arrow, Backspace, BackspaceEmpty, Bottom, Copy, CtrlSubmit, Cut, Delete, DeleteWordLeft,
    DeleteWordRight, Down, Edge, End, Home, Left, Newline, PageDown, PageUp, Paste, Right,
    SelectAll, SelectBottom, SelectDown, SelectEnd, SelectHome, SelectLeft, SelectPageDown,
    SelectPageUp, SelectRight, SelectTop, SelectUp, SelectWordLeft, SelectWordRight, Submit,
    Submitted, SubmittedBack, Tab, Tabbed, TextArea, Top, Up, WordLeft, WordRight,
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

    pub(crate) fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        let to = if self.selected.is_empty() {
            self.previous_boundary(self.cursor())
        } else {
            self.selected.start
        };
        self.move_to(to, window, cx);
    }

    pub(crate) fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        let to = if self.selected.is_empty() {
            self.next_boundary(self.selected.end)
        } else {
            self.selected.end
        };
        self.move_to(to, window, cx);
    }

    pub(crate) fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        if self.single_line || self.menu_keys {
            return cx.emit(Arrow(-1));
        }
        let first_row = self
            .position_for(self.cursor())
            .is_none_or(|at| at.y < self.line_height / 2.);
        let to = self.vertical(-1.).unwrap_or(0);
        self.move_to(to, window, cx);
        if first_row {
            cx.emit(Edge(-1));
        }
    }

    pub(crate) fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        if self.single_line || self.menu_keys {
            return cx.emit(Arrow(1));
        }
        let last_row = match (self.position_for(self.cursor()), self.lines.last()) {
            (Some(at), Some(last)) => {
                let bottom = last.top + last.layout.size(self.line_height).height;
                at.y + self.line_height * 1.5 > bottom
            }
            _ => true,
        };
        let to = self.vertical(1.).unwrap_or(self.content.len());
        self.move_to(to, window, cx);
        if last_row {
            cx.emit(Edge(1));
        }
    }

    pub(crate) fn select_left(
        &mut self,
        _: &SelectLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_by_user(self.previous_boundary(self.cursor()), window, cx);
    }

    pub(crate) fn select_right(
        &mut self,
        _: &SelectRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_by_user(self.next_boundary(self.cursor()), window, cx);
    }

    pub(crate) fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        let to = self.vertical(-1.).unwrap_or(0);
        self.select_by_user(to, window, cx);
    }

    pub(crate) fn select_down(
        &mut self,
        _: &SelectDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let to = self.vertical(1.).unwrap_or(self.content.len());
        self.select_by_user(to, window, cx);
    }

    pub(crate) fn select_all(
        &mut self,
        _: &SelectAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_everything(window, cx);
        self.settle(cx);
    }

    /// Selects the whole text, so typing replaces it.
    pub fn select_everything(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
        self.select_to(self.content.len(), window, cx);
    }

    /// The first character of the cursor's line past its indent, or the line's start when the
    /// cursor is there already.
    fn smart_home(&self) -> usize {
        let line = self.line_at(self.cursor());
        let text = &self.content[line.clone()];
        let first = line.start + text.len() - text.trim_start().len();
        if self.cursor() == first {
            line.start
        } else {
            first
        }
    }

    pub(crate) fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.smart_home(), window, cx);
    }

    /// The end of the cursor's line.
    pub(crate) fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.line_at(self.cursor()).end, window, cx);
    }

    pub(crate) fn select_home(
        &mut self,
        _: &SelectHome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_by_user(self.smart_home(), window, cx);
    }

    pub(crate) fn select_end(
        &mut self,
        _: &SelectEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_by_user(self.line_at(self.cursor()).end, window, cx);
    }

    pub(crate) fn top(&mut self, _: &Top, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
    }

    pub(crate) fn bottom(&mut self, _: &Bottom, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), window, cx);
    }

    pub(crate) fn select_top(
        &mut self,
        _: &SelectTop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_by_user(0, window, cx);
    }

    pub(crate) fn select_bottom(
        &mut self,
        _: &SelectBottom,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_by_user(self.content.len(), window, cx);
    }

    /// The start of the word before `offset`, past any blanks in between.
    fn word_start_before(&self, offset: usize) -> usize {
        let before = self.content[..offset].trim_end();
        if before.is_empty() {
            return 0;
        }
        self.word_at(before.len() - before.chars().next_back().map_or(0, char::len_utf8))
            .start
    }

    /// The end of the word after `offset`, past any blanks in between.
    fn word_end_after(&self, offset: usize) -> usize {
        let after = &self.content[offset..];
        let blanks = after.len() - after.trim_start().len();
        if offset + blanks >= self.content.len() {
            return self.content.len();
        }
        self.word_at(offset + blanks).end
    }

    pub(crate) fn word_left(&mut self, _: &WordLeft, window: &mut Window, cx: &mut Context<Self>) {
        let from = if self.selected.is_empty() {
            self.cursor()
        } else {
            self.selected.start
        };
        self.move_to(self.word_start_before(from), window, cx);
    }

    pub(crate) fn word_right(
        &mut self,
        _: &WordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let from = if self.selected.is_empty() {
            self.cursor()
        } else {
            self.selected.end
        };
        self.move_to(self.word_end_after(from), window, cx);
    }

    pub(crate) fn select_word_left(
        &mut self,
        _: &SelectWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_by_user(self.word_start_before(self.cursor()), window, cx);
    }

    pub(crate) fn select_word_right(
        &mut self,
        _: &SelectWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_by_user(self.word_end_after(self.cursor()), window, cx);
    }

    pub(crate) fn delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.content.is_empty() {
            return cx.emit(BackspaceEmpty);
        }
        if self.selected.is_empty() {
            self.select_to(self.word_start_before(self.cursor()), window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(crate) fn delete_word_right(
        &mut self,
        _: &DeleteWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected.is_empty() {
            self.select_to(self.word_end_after(self.cursor()), window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    /// How many rows Page Up and Page Down move: the rows the field's box shows, less one so
    /// a row stays in view.
    fn page_rows(&self) -> f32 {
        let shown = self.scroll.bounds().size.height;
        let shown = if shown > px(0.) {
            shown
        } else {
            self.bounds.map_or(px(0.), |b| b.size.height)
        };
        (shown / self.line_height - 1.).floor().max(1.)
    }

    pub(crate) fn page_up(&mut self, _: &PageUp, window: &mut Window, cx: &mut Context<Self>) {
        let to = self.vertical(-self.page_rows()).unwrap_or(0);
        self.move_to(to, window, cx);
    }

    pub(crate) fn page_down(&mut self, _: &PageDown, window: &mut Window, cx: &mut Context<Self>) {
        let to = self
            .vertical(self.page_rows())
            .unwrap_or(self.content.len());
        self.move_to(to, window, cx);
    }

    pub(crate) fn select_page_up(
        &mut self,
        _: &SelectPageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let to = self.vertical(-self.page_rows()).unwrap_or(0);
        self.select_by_user(to, window, cx);
    }

    pub(crate) fn select_page_down(
        &mut self,
        _: &SelectPageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let to = self
            .vertical(self.page_rows())
            .unwrap_or(self.content.len());
        self.select_by_user(to, window, cx);
    }

    pub(crate) fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.content.is_empty() {
            return cx.emit(BackspaceEmpty);
        }
        if self.selected.is_empty() {
            self.select_to(self.previous_boundary(self.cursor()), window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(crate) fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.select_to(self.next_boundary(self.cursor()), window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(crate) fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.single_line {
            return cx.emit(SubmittedBack);
        }
        self.replace_text_in_range(None, "\n", window, cx);
    }

    pub(crate) fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        if self.enter_newline {
            return self.replace_text_in_range(None, "\n", window, cx);
        }
        cx.emit(Submitted);
    }

    pub(crate) fn ctrl_submit(&mut self, _: &CtrlSubmit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(Submitted);
    }

    /// Tab picks from an open menu; otherwise it moves the focus on.
    pub(crate) fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        if self.menu_keys {
            cx.emit(Tabbed);
        } else {
            cx.propagate();
        }
    }

    pub(crate) fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.insert_pasted(&text, window, cx);
        }
    }

    /// Puts pasted `text` in place of the selection, as one undo step of its own.
    pub(crate) fn insert_pasted(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.replace("\r\n", "\n");
        // A one-line field has no line to break, and a token copied with its line end is not
        // the token.
        let text = if self.single_line {
            text.trim_matches('\n').replace('\n', " ")
        } else {
            text
        };
        self.history.typing_at = None;
        self.replace_text_in_range(None, &text, window, cx);
        self.history.typing_at = None;
    }

    pub(crate) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        // With nothing selected here, Ctrl+C copies what is marked in the chat.
        if self.selected.is_empty() {
            cx.propagate();
            return;
        }
        // A masked field's text stays in it, as its menu says.
        if !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected.clone()].to_string(),
            ));
        }
    }

    pub(crate) fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected.clone()].to_string(),
            ));
            self.history.typing_at = None;
            self.replace_text_in_range(None, "", window, cx);
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

    pub(crate) fn offset_to_utf16(&self, offset: usize) -> usize {
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

    pub(crate) fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub(crate) fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}
