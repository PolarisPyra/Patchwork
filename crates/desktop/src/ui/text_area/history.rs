//! Undo and redo: the text and its selection before each change, with a run of typing kept as one
//! step.

// Rust guideline compliant 2026-11-10

use std::ops::Range;

use gpui::{Window, prelude::*};

use super::state::{Changed, Redo, TextArea, Undo};

/// How many changes Undo can step back through.
const LIMIT: usize = 200;

/// The text and selection as they were before a change.
pub(super) struct Snapshot {
    content: String,
    selected: Range<usize>,
}

/// The changes Undo and Redo step through.
#[derive(Default)]
pub(super) struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Where the last typed character ended, while typing goes on: the next one joins its step.
    pub(super) typing_at: Option<usize>,
}

impl TextArea {
    /// Keeps the text as it is now, before a change of `range` to `text`, so Undo can bring it
    /// back. Characters typed one after another without a break, a space or a line end make one
    /// step.
    pub(super) fn record(&mut self, range: &Range<usize>, text: &str) {
        // Nothing changes. The input method rewrites its marked text as it composes: the step
        // began with it.
        if (range.is_empty() && text.is_empty()) || self.marked.is_some() {
            return;
        }
        let typing =
            range.is_empty() && text.chars().count() == 1 && !text.chars().any(char::is_whitespace);
        let joins = typing && self.history.typing_at == Some(range.start);
        self.history.typing_at = typing.then(|| range.start + text.len());
        if joins {
            return;
        }
        self.history.redo.clear();
        self.history.undo.push(Snapshot {
            content: self.content.clone(),
            selected: self.selected.clone(),
        });
        if self.history.undo.len() > LIMIT {
            self.history.undo.remove(0);
        }
    }

    /// Forgets every change, as when the field is given another text.
    pub(super) fn forget_history(&mut self) {
        self.history = History::default();
    }

    pub(super) fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(before) = self.history.undo.pop() {
            let now = self.restore(before, window, cx);
            self.history.redo.push(now);
        }
    }

    pub(super) fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(after) = self.history.redo.pop() {
            let now = self.restore(after, window, cx);
            self.history.undo.push(now);
        }
    }

    /// Puts `snapshot` back and returns what it replaced.
    fn restore(&mut self, snapshot: Snapshot, window: &Window, cx: &mut Context<Self>) -> Snapshot {
        let now = Snapshot {
            content: std::mem::replace(&mut self.content, snapshot.content),
            selected: self.selected.clone(),
        };
        let len = self.content.len();
        self.selected = snapshot.selected.start.min(len)..snapshot.selected.end.min(len);
        self.reversed = false;
        self.marked = None;
        self.history.typing_at = None;
        cx.emit(Changed);
        window.notify_text_input_changed(&self.focus);
        cx.notify();
        now
    }
}
