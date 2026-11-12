//! What the keys and the mouse do to the text field: moving, selecting, editing, pasting.

// Rust guideline compliant 2026-10-09

use std::ops::Range;

use gpui::{
    ClipboardItem, EntityInputHandler, FocusHandle, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, Window, point, prelude::*, px,
};
use crate::ui::menu::{Dismissed, Menu, MenuItem};
use super::state::{
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
        let first_row = self
            .position_for(self.cursor())
            .is_none_or(|at| at.y < self.line_height / 2.);
        let to = self.vertical(-1.).unwrap_or(0);
        self.move_to(to, window, cx);
        if first_row {
            cx.emit(Edge(-1));
        }
    }

    pub(super) fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
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
        self.move_to(self.line_at(self.cursor()).start, window, cx);
    }

    /// The end of the cursor's line.
    pub(super) fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.line_at(self.cursor()).end, window, cx);
    }

    pub(super) fn select_home(
        &mut self,
        _: &SelectHome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.line_at(self.cursor()).start, window, cx);
    }

    pub(super) fn select_end(
        &mut self,
        _: &SelectEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.line_at(self.cursor()).end, window, cx);
    }

    pub(super) fn top(&mut self, _: &Top, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
    }

    pub(super) fn bottom(&mut self, _: &Bottom, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), window, cx);
    }

    pub(super) fn select_top(
        &mut self,
        _: &SelectTop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(0, window, cx);
    }

    pub(super) fn select_bottom(
        &mut self,
        _: &SelectBottom,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.content.len(), window, cx);
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

    pub(super) fn word_left(&mut self, _: &WordLeft, window: &mut Window, cx: &mut Context<Self>) {
        let from = if self.selected.is_empty() {
            self.cursor()
        } else {
            self.selected.start
        };
        self.move_to(self.word_start_before(from), window, cx);
    }

    pub(super) fn word_right(
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

    pub(super) fn select_word_left(
        &mut self,
        _: &SelectWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.word_start_before(self.cursor()), window, cx);
    }

    pub(super) fn select_word_right(
        &mut self,
        _: &SelectWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.word_end_after(self.cursor()), window, cx);
    }

    pub(super) fn delete_word_left(
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

    pub(super) fn delete_word_right(
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

    pub(super) fn page_up(&mut self, _: &PageUp, window: &mut Window, cx: &mut Context<Self>) {
        let to = self.vertical(-self.page_rows()).unwrap_or(0);
        self.move_to(to, window, cx);
    }

    pub(super) fn page_down(&mut self, _: &PageDown, window: &mut Window, cx: &mut Context<Self>) {
        let to = self
            .vertical(self.page_rows())
            .unwrap_or(self.content.len());
        self.move_to(to, window, cx);
    }

    pub(super) fn select_page_up(
        &mut self,
        _: &SelectPageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let to = self.vertical(-self.page_rows()).unwrap_or(0);
        self.select_to(to, window, cx);
    }

    pub(super) fn select_page_down(
        &mut self,
        _: &SelectPageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let to = self
            .vertical(self.page_rows())
            .unwrap_or(self.content.len());
        self.select_to(to, window, cx);
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

    /// A right click: the caret moves there unless it is on the selection, and a menu opens to
    /// cut, copy, paste or select all. Each runs as its key would, so the owner's own handling,
    /// such as the composer attaching a pasted picture, still applies.
    pub(crate) fn open_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.offset_at(event.position);
        window.focus(&self.focus, cx);
        if !(self.selected.start <= offset && offset <= self.selected.end) {
            self.move_to(offset, window, cx);
        }
        let can_copy = !self.selected.is_empty() && !self.masked;
        let item = |label: &'static str, action: Box<dyn gpui::Action>, focus: &FocusHandle| {
            let focus = focus.clone();
            MenuItem::new(label, move |window, cx| {
                window.focus(&focus, cx);
                focus.dispatch_action(action.as_ref(), window, cx);
            })
        };
        let mut items = Vec::new();
        if can_copy {
            items.push(item("Cut", Box::new(Cut), &self.focus));
            items.push(item("Copy", Box::new(Copy), &self.focus).icon("copy"));
        }
        items.push(item("Paste", Box::new(Paste), &self.focus));
        items.push(item("Select all", Box::new(SelectAll), &self.focus).separated());
        let menu = Menu::open("Edit", items, window, cx);
        cx.subscribe_in(&menu, window, |this, _, _: &Dismissed, window, cx| {
            this.menu = None;
            window.focus(&this.focus, cx);
            cx.notify();
        })
        .detach();
        self.menu = Some((menu, event.position));
        cx.stop_propagation();
        cx.notify();
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
