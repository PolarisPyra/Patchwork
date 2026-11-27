//! The text field itself: its text, its selection, and where each offset is drawn.

// Rust guideline compliant 2026-10-09

use std::ops::Range;

use gpui::{
    Bounds, Entity, EventEmitter, FocusHandle, Hsla, KeyBinding, Pixels, Point, ScrollHandle,
    SharedString, Task, Window, WrappedLine, actions, point, prelude::*, px,
};
use unicode_segmentation::UnicodeSegmentation;

use super::history::History;
use super::mouse::Grain;
use patchwork_ui_markdown::selection::{Owner, claim, owner, to_primary};

actions!(
    text_area,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Newline,
        Submit,
        CtrlSubmit,
        Tab,
        Undo,
        Redo,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        DeleteWordLeft,
        DeleteWordRight,
        SelectHome,
        SelectEnd,
        Top,
        Bottom,
        SelectTop,
        SelectBottom,
        PageUp,
        PageDown,
        SelectPageUp,
        SelectPageDown,
    ]
);

/// The keys the field answers to.
#[must_use]
pub fn bindings() -> Vec<KeyBinding> {
    let context = Some("TextArea");
    vec![
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("shift-up", SelectUp, context),
        KeyBinding::new("shift-down", SelectDown, context),
        KeyBinding::new("ctrl-a", SelectAll, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("ctrl-v", Paste, context),
        KeyBinding::new("ctrl-x", Cut, context),
        KeyBinding::new("ctrl-c", Copy, context),
        KeyBinding::new("shift-enter", Newline, context),
        KeyBinding::new("enter", Submit, context),
        KeyBinding::new("ctrl-enter", CtrlSubmit, context),
        KeyBinding::new("tab", Tab, context),
        KeyBinding::new("ctrl-z", Undo, context),
        KeyBinding::new("ctrl-y", Redo, context),
        KeyBinding::new("ctrl-left", WordLeft, context),
        KeyBinding::new("ctrl-right", WordRight, context),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, context),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, context),
        KeyBinding::new("ctrl-backspace", DeleteWordLeft, context),
        KeyBinding::new("ctrl-delete", DeleteWordRight, context),
        KeyBinding::new("shift-home", SelectHome, context),
        KeyBinding::new("shift-end", SelectEnd, context),
        KeyBinding::new("ctrl-home", Top, context),
        KeyBinding::new("ctrl-end", Bottom, context),
        KeyBinding::new("ctrl-shift-home", SelectTop, context),
        KeyBinding::new("ctrl-shift-end", SelectBottom, context),
        KeyBinding::new("pageup", PageUp, context),
        KeyBinding::new("pagedown", PageDown, context),
        KeyBinding::new("shift-pageup", SelectPageUp, context),
        KeyBinding::new("shift-pagedown", SelectPageDown, context),
    ]
}

/// Enter was pressed: the owner sends the text.
#[derive(Debug)]
pub struct Submitted;

/// Shift+Enter in a one-line field, which has no lines to break: its owner steps back.
#[derive(Debug)]
pub struct SubmittedBack;

/// The text changed, by typing or by a button.
#[derive(Debug)]
pub struct Changed;

/// Backspace in an empty field, which has nothing to delete: its owner may take back what came
/// before it.
#[derive(Debug)]
pub struct BackspaceEmpty;

/// Up (-1) or Down (1) in a one-line field, which has no lines to move between: its owner moves a
/// list's highlight instead.
#[derive(Debug)]
pub struct Arrow(pub isize);

/// Up on the first row (-1) or Down on the last (1) of a field with lines, after the caret has
/// moved to the start or the end: its owner may bring back an earlier message.
#[derive(Debug)]
pub struct Edge(pub isize);

/// Tab while a menu has the keys: its owner picks the highlighted entry.
#[derive(Debug)]
pub struct Tabbed;

/// One `\n`-separated line as last drawn: where it starts in the text, its wrapped layout, and its
/// top relative to the field.
#[derive(Debug)]
pub(crate) struct DrawnLine {
    pub(crate) start: usize,
    pub(crate) layout: WrappedLine,
    pub(crate) top: Pixels,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate piece of view state"
)]
#[derive(Debug)]
pub struct TextArea {
    pub(crate) focus: FocusHandle,
    pub content: String,
    pub placeholder: SharedString,
    pub(crate) label: SharedString,
    pub(crate) selected: Range<usize>,
    pub(crate) reversed: bool,
    pub(crate) marked: Option<Range<usize>>,
    pub(crate) lines: Vec<DrawnLine>,
    pub(crate) bounds: Option<Bounds<Pixels>>,
    pub(crate) line_height: Pixels,
    pub(crate) selecting: bool,
    /// What a drag selects by, and what the press that began it selected.
    pub(crate) grain: Grain,
    pub(crate) anchor: Range<usize>,
    /// Where a press on the selection landed: a drag from it moves the selected text.
    pub(crate) moving: Option<usize>,
    /// Where the moved text would go, while it is dragged.
    pub(crate) drop_at: Option<usize>,
    /// Where the pointer last was in a drag, and the timer that scrolls while it is outside.
    pub(crate) pointer: Option<Point<Pixels>>,
    pub(crate) scrolling: Option<Task<()>>,
    /// The colors its owner draws it in. Each one left unset is the theme's, read as it is
    /// drawn, so the field follows the theme when it changes: the text color, `text_subtle`, and
    /// the accent at 30%.
    pub text_color: Option<Hsla>,
    pub placeholder_color: Option<Hsla>,
    pub selection_color: Option<Hsla>,
    /// One line, as a search field: Enter submits, and Up and Down are the owner's.
    pub single_line: bool,
    /// Enter starts a new line and Ctrl+Enter submits, as the commit message box does.
    pub enter_newline: bool,
    /// Drawn as dots, as a secret is, rather than as what was typed.
    pub masked: bool,
    /// Markdown is styled as it is typed, with its marks dimmed.
    pub markdown: bool,
    /// A menu above the field has Up, Down and Tab, which go to the owner as [`Arrow`] and
    /// [`Tabbed`].
    pub menu_keys: bool,
    /// The right-click menu, while open, and where it opened.
    pub(crate) menu: Option<(Entity<patchwork_ui_components::menu::Menu>, Point<Pixels>)>,
    /// The box the field scrolls in, when its owner tracks one: it follows the caret.
    pub scroll: ScrollHandle,
    /// The caret and text length the box last followed, so it moves only when they change.
    pub(crate) revealed: (usize, usize),
    /// How far a single-line field's text is scrolled left, so the caret stays in view.
    pub(crate) scroll_x: Pixels,
    /// What Undo and Redo step through.
    pub(crate) history: History,
}

/// What a masked field draws for each character.
const DOT: char = '•';

impl EventEmitter<Submitted> for TextArea {}
impl EventEmitter<SubmittedBack> for TextArea {}
impl EventEmitter<Changed> for TextArea {}
impl EventEmitter<BackspaceEmpty> for TextArea {}
impl EventEmitter<Arrow> for TextArea {}
impl EventEmitter<Tabbed> for TextArea {}
impl EventEmitter<Edge> for TextArea {}

impl TextArea {
    pub fn new(
        placeholder: impl Into<SharedString>,
        label: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        // Another selection made after this one takes its place: only the caret is left.
        cx.observe_global::<Owner>(|this, cx| {
            if !this.selected.is_empty() && owner(cx) != Owner::Field(cx.entity_id()) {
                let at = this.cursor();
                this.selected = at..at;
                this.reversed = false;
                cx.notify();
            }
        })
        .detach();
        Self {
            focus: cx.focus_handle(),
            content: String::new(),
            placeholder: placeholder.into(),
            label: label.into(),
            selected: 0..0,
            reversed: false,
            marked: None,
            lines: Vec::new(),
            bounds: None,
            line_height: px(20.),
            selecting: false,
            grain: Grain::default(),
            anchor: 0..0,
            moving: None,
            drop_at: None,
            pointer: None,
            scrolling: None,
            text_color: None,
            placeholder_color: None,
            selection_color: None,
            single_line: false,
            enter_newline: false,
            masked: false,
            markdown: false,
            menu_keys: false,
            menu: None,
            scroll: ScrollHandle::new(),
            revealed: (0, 0),
            scroll_x: px(0.),
            history: History::default(),
        }
    }

    /// The text as drawn: the content, or a dot for each of its characters.
    pub(crate) fn shown(&self) -> String {
        if self.masked {
            self.content.chars().map(|_| DOT).collect()
        } else {
            self.content.clone()
        }
    }

    /// Where content offset `offset` falls in the drawn text.
    pub(crate) fn shown_offset(&self, offset: usize) -> usize {
        if self.masked {
            self.content[..offset].chars().count() * DOT.len_utf8()
        } else {
            offset
        }
    }

    /// The content offset of drawn offset `shown`.
    fn content_offset(&self, shown: usize) -> usize {
        if self.masked {
            self.content
                .char_indices()
                .nth(shown / DOT.len_utf8())
                .map_or(self.content.len(), |(i, _)| i)
        } else {
            shown
        }
    }

    pub fn clear(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.record(&(0..self.content.len()), "");
        self.content.clear();
        cx.emit(Changed);
        self.selected = 0..0;
        self.reversed = false;
        self.marked = None;
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    /// Replaces the text, with the cursor at its end.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.forget_history();
        self.content = text.to_string();
        self.selected = text.len()..text.len();
        self.reversed = false;
        self.marked = None;
        cx.notify();
    }

    /// Wraps the selection in `before` and `after`, keeping it selected, as the format bar does.
    pub fn surround(&mut self, before: &str, after: &str, window: &Window, cx: &mut Context<Self>) {
        let range = self.selected.clone();
        self.record(&range, before);
        self.content.insert_str(range.end, after);
        self.content.insert_str(range.start, before);
        // Its range was measured before the text moved.
        self.marked = None;
        cx.emit(Changed);
        self.selected = range.start + before.len()..range.end + before.len();
        self.reversed = false;
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    /// Starts each line the selection touches, or the cursor's line, with `prefix`, as the list
    /// buttons do; when they all start with it already, it comes off them instead. One undo step.
    pub fn prefix_line(&mut self, prefix: &str, window: &Window, cx: &mut Context<Self>) {
        let (from, to) = (self.selected.start, self.selected.end);
        let start = self.content[..from].rfind('\n').map_or(0, |i| i + 1);
        // A selection that ends at the start of a line leaves that line out.
        let last = if to > from && self.content[..to].ends_with('\n') {
            to - 1
        } else {
            to
        };
        let end = self.content[last..]
            .find('\n')
            .map_or(self.content.len(), |i| last + i);
        let block = &self.content[start..end];
        let old_len = block.len();
        let remove = block.split('\n').all(|line| line.starts_with(prefix));
        let lines: Vec<String> = block
            .split('\n')
            .map(|line| match line.strip_prefix(prefix) {
                Some(rest) if remove => rest.to_string(),
                _ if remove => line.to_string(),
                Some(_) => line.to_string(),
                None => format!("{prefix}{line}"),
            })
            .collect();
        let text = lines.join("\n");
        let range = start..end;
        self.record(&range, &text);
        self.content.replace_range(range, &text);
        // Its range was measured before the text moved.
        self.marked = None;
        cx.emit(Changed);
        self.selected = if from == to {
            // One line: the caret keeps its place in the line's text.
            let at = if remove {
                from.saturating_sub(prefix.len()).max(start)
            } else {
                from + text.len() - old_len
            };
            at..at
        } else {
            start..start + text.len()
        };
        self.reversed = false;
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    /// Where the caret is, unless text is selected.
    #[must_use]
    pub fn caret(&self) -> Option<usize> {
        self.selected.is_empty().then_some(self.selected.end)
    }

    /// Replaces `range` of the text with `text`, leaving the cursor after it.
    pub fn replace(
        &mut self,
        range: Range<usize>,
        text: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.record(&range, text);
        self.content.replace_range(range.clone(), text);
        cx.emit(Changed);
        let end = range.start + text.len();
        self.selected = end..end;
        self.reversed = false;
        self.marked = None;
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    pub(crate) fn cursor(&self) -> usize {
        if self.reversed {
            self.selected.start
        } else {
            self.selected.end
        }
    }

    pub fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        self.history.typing_at = None;
        self.selected = offset..offset;
        self.reversed = false;
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    pub(crate) fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        if self.reversed {
            self.selected.start = offset;
        } else {
            self.selected.end = offset;
        }
        if self.selected.end < self.selected.start {
            self.reversed = !self.reversed;
            self.selected = self.selected.end..self.selected.start;
        }
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    /// Selects to `offset` as the user asked, which makes this the latest selection.
    pub(crate) fn select_by_user(
        &mut self,
        offset: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(offset, window, cx);
        self.settle(cx);
    }

    /// Makes a selection the user made the latest, and the primary selection unless it is a
    /// secret.
    pub(crate) fn settle(&self, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            return;
        }
        claim(Owner::Field(cx.entity_id()), cx);
        if !self.masked {
            to_primary(&self.content[self.selected.clone()], cx);
        }
    }

    /// The word around `offset`, or the run of blanks or of punctuation it is in.
    pub(crate) fn word_at(&self, offset: usize) -> Range<usize> {
        patchwork_ui_markdown::selection::word_at(&self.content, offset)
    }

    /// The line `offset` is on, without its line end.
    pub(crate) fn line_at(&self, offset: usize) -> Range<usize> {
        let offset = offset.min(self.content.len());
        let start = self.content[..offset].rfind('\n').map_or(0, |i| i + 1);
        let end = self.content[offset..]
            .find('\n')
            .map_or(self.content.len(), |i| offset + i);
        start..end
    }

    pub(crate) fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(i, _)| (i < offset).then_some(i))
            .unwrap_or(0)
    }

    pub(crate) fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(i, _)| (i > offset).then_some(i))
            .unwrap_or(self.content.len())
    }

    /// Where `offset` was drawn, relative to the field.
    pub(crate) fn position_for(&self, offset: usize) -> Option<Point<Pixels>> {
        let offset = self.shown_offset(offset);
        let line = self.lines.iter().rev().find(|line| line.start <= offset)?;
        let at = line
            .layout
            .position_for_index(offset - line.start, self.line_height)?;
        Some(point(at.x, at.y + line.top))
    }

    /// The text offset nearest `position`, relative to the field.
    pub(crate) fn offset_for(&self, position: Point<Pixels>) -> usize {
        let Some(line) = self
            .lines
            .iter()
            .rev()
            .find(|line| line.top <= position.y)
            .or(self.lines.first())
        else {
            return 0;
        };
        let local = point(position.x, position.y - line.top);
        let index = match line
            .layout
            .closest_index_for_position(local, self.line_height)
        {
            Ok(i) | Err(i) => i,
        };
        self.content_offset(line.start + index)
            .min(self.content.len())
    }
}
