//! What the mouse does to the text field: placing the caret, selecting by letter, word or line,
//! dragging past the edge to scroll, moving the selected text, pasting the primary selection, and
//! the right-click menu.

// Rust guideline compliant 2026-10-09

use std::ops::Range;
use std::time::Duration;

use gpui::{
    Bounds, FocusHandle, MouseDownEvent, MouseUpEvent, Pixels, Point, Window, point, prelude::*, px,
};

use super::field::{Changed, Copy, Cut, Paste, SelectAll, TextArea};
use crate::controls::menu::{Dismissed, Menu, MenuItem};
use crate::text::selection::{Owner, claim, from_primary};

/// How often a drag held past the field's edge scrolls it on.
const SCROLL_EVERY: Duration = Duration::from_millis(30);

/// What a drag selects by: the press's count picks it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Grain {
    #[default]
    Letter,
    Word,
    Line,
}

impl TextArea {
    pub(super) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.offset_at(event.position);
        self.pointer = None;
        // A press inside the selection may drag it somewhere else; let go without moving, and
        // the caret goes there.
        if event.click_count == 1
            && !event.modifiers.shift
            && !self.masked
            && self.selected.start < offset
            && offset < self.selected.end
        {
            self.moving = Some(offset);
            self.drop_at = None;
            return cx.notify();
        }
        self.selecting = true;
        // A second press selects the word under it, and a third its line; a drag from there
        // goes on by words or by lines.
        if event.click_count > 1 {
            self.grain = if event.click_count == 2 {
                Grain::Word
            } else {
                Grain::Line
            };
            self.anchor = self.unit_at(offset);
            self.selected = self.anchor.clone();
            self.reversed = false;
            window.notify_text_selection_changed(&self.focus);
            self.settle(cx);
            return cx.notify();
        }
        self.grain = Grain::Letter;
        if event.modifiers.shift {
            self.select_by_user(offset, window, cx);
        } else {
            self.move_to(offset, window, cx);
        }
    }

    /// The pointer moved anywhere in the window during a press on the field.
    pub(super) fn drag(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pointer = Some(position);
        self.drag_to(position, window, cx);
        self.keep_scrolling(window, cx);
    }

    /// The press on the field ended, wherever the pointer is.
    pub(super) fn end_drag(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scrolling = None;
        self.pointer = None;
        if let Some(pressed) = self.moving.take() {
            match self.drop_at.take() {
                Some(to) if to < self.selected.start || to > self.selected.end => {
                    self.move_selected(to, event.modifiers.control, window, cx);
                }
                _ => self.move_to(pressed, window, cx),
            }
            return cx.notify();
        }
        if std::mem::take(&mut self.selecting) {
            self.settle(cx);
        }
    }

    fn drag_to(&mut self, position: Point<Pixels>, window: &Window, cx: &mut Context<Self>) {
        let offset = self.offset_at(position);
        if self.moving.is_some() {
            self.drop_at = Some(offset);
            return cx.notify();
        }
        if !self.selecting {
            return;
        }
        if self.grain == Grain::Letter {
            self.select_to(offset, window, cx);
        } else {
            let reach = self.unit_at(offset);
            self.reversed = reach.start < self.anchor.start;
            self.selected = self.anchor.start.min(reach.start)..self.anchor.end.max(reach.end);
            window.notify_text_selection_changed(&self.focus);
            cx.notify();
        }
        if !self.selected.is_empty() {
            claim(Owner::Field(cx.entity_id()), cx);
        }
    }

    /// The word or line at `offset`, or `offset` alone, by the drag's grain.
    fn unit_at(&self, offset: usize) -> Range<usize> {
        match self.grain {
            Grain::Letter => offset..offset,
            Grain::Word => self.word_at(offset),
            Grain::Line => self.line_at(offset),
        }
    }

    /// The part of the field in view, in the window.
    fn view(&self) -> Option<Bounds<Pixels>> {
        let drawn = self.bounds?;
        let field = Bounds::new(drawn.origin + point(self.scroll_x, px(0.)), drawn.size);
        let view = self.scroll.bounds();
        Some(if view.size.height > px(0.) {
            view.intersect(&field)
        } else {
            field
        })
    }

    /// While the pointer is held past the field's edge, the drag goes on at a steady pace, and
    /// the field scrolls to follow the end of the selection.
    fn keep_scrolling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let outside = self
            .pointer
            .zip(self.view())
            .is_some_and(|(at, view)| !view.contains(&at));
        if !outside {
            self.scrolling = None;
            return;
        }
        if self.scrolling.is_some() {
            return;
        }
        self.scrolling = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(SCROLL_EVERY).await;
                let more = this
                    .update_in(cx, |this, window, cx| {
                        let Some(at) = this
                            .pointer
                            .filter(|_| this.selecting || this.moving.is_some())
                        else {
                            return false;
                        };
                        this.drag_to(at, window, cx);
                        this.view().is_some_and(|view| !view.contains(&at))
                    })
                    .unwrap_or(false);
                if !more {
                    break;
                }
            }
        }));
    }

    /// Moves the selected text to `to`, or copies it there, as one undo step, and keeps it
    /// selected.
    fn move_selected(&mut self, to: usize, copy: bool, window: &Window, cx: &mut Context<Self>) {
        let range = self.selected.clone();
        let text = self.content[range.clone()].to_string();
        self.history.typing_at = None;
        self.record(&range, "");
        self.selected = if copy {
            self.content.insert_str(to, &text);
            to..to + text.len()
        } else if to > range.end {
            self.content.insert_str(to, &text);
            self.content.replace_range(range, "");
            to - text.len()..to
        } else {
            self.content.replace_range(range, "");
            self.content.insert_str(to, &text);
            to..to + text.len()
        };
        self.history.typing_at = None;
        self.reversed = false;
        self.marked = None;
        cx.emit(Changed);
        window.notify_text_input_changed(&self.focus);
        self.settle(cx);
        cx.notify();
    }

    /// A middle click pastes the primary selection where it lands.
    pub(super) fn paste_primary(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = from_primary(cx) else {
            return;
        };
        window.focus(&self.focus, cx);
        self.move_to(self.offset_at(event.position), window, cx);
        self.insert_pasted(&text, window, cx);
        cx.stop_propagation();
    }

    /// A right click: the caret moves there unless it is on the selection, and a menu opens to
    /// cut, copy, paste or select all. Each runs as its key would, so the owner's own handling,
    /// such as the composer attaching a pasted picture, still applies.
    pub fn open_menu(
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
            items.push(item("Cut", Box::new(Cut), &self.focus).icon("scissors"));
            items.push(item("Copy", Box::new(Copy), &self.focus).icon("copy"));
        }
        items.push(item("Paste", Box::new(Paste), &self.focus).icon("clipboard-text"));
        items.push(
            item("Select all", Box::new(SelectAll), &self.focus)
                .icon("selection-all")
                .separated(),
        );
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
}
