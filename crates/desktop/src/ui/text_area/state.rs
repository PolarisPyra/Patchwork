//! The text field itself: its text, its selection, and where each offset is drawn.

// Rust guideline compliant 2026-10-09

use std::ops::Range;

use gpui::{
    Bounds, Entity, EventEmitter, FocusHandle, Hsla, KeyBinding, Pixels, Point, SharedString,
    Window, WrappedLine, actions, point, prelude::*, px,
};
use unicode_segmentation::UnicodeSegmentation;

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
    ]
);

/// The keys the field answers to.
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
    ]
}

/// Enter was pressed: the owner sends the text.
pub struct Submitted;

/// Shift+Enter in a one-line field, which has no lines to break: its owner steps back.
pub struct SubmittedBack;

/// The text changed, by typing or by a button.
pub struct Changed;

/// Backspace in an empty field, which has nothing to delete: its owner may take back what came
/// before it.
pub struct BackspaceEmpty;

/// Up (-1) or Down (1) in a one-line field, which has no lines to move between: its owner moves a
/// list's highlight instead.
pub struct Arrow(pub isize);

/// Tab while a menu has the keys: its owner picks the highlighted entry.
pub struct Tabbed;

/// One `\n`-separated line as last drawn: where it starts in the text, its wrapped layout, and its
/// top relative to the field.
pub(super) struct DrawnLine {
    pub(super) start: usize,
    pub(super) layout: WrappedLine,
    pub(super) top: Pixels,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate piece of view state"
)]
pub struct TextArea {
    pub(super) focus: FocusHandle,
    pub content: String,
    pub placeholder: SharedString,
    pub(super) label: SharedString,
    pub(super) selected: Range<usize>,
    pub(super) reversed: bool,
    pub(super) marked: Option<Range<usize>>,
    pub(super) lines: Vec<DrawnLine>,
    pub(super) bounds: Option<Bounds<Pixels>>,
    pub(super) line_height: Pixels,
    pub(super) selecting: bool,
    pub text_color: Hsla,
    pub placeholder_color: Hsla,
    pub selection_color: Hsla,
    /// One line, as a search field: Enter submits, and Up and Down are the owner's.
    pub single_line: bool,
    /// Enter starts a new line and Ctrl+Enter submits, as the commit message box does.
    pub enter_newline: bool,
    /// Drawn as dots, as a secret is, rather than as what was typed.
    pub masked: bool,
    /// A menu above the field has Up, Down and Tab, which go to the owner as [`Arrow`] and
    /// [`Tabbed`].
    pub menu_keys: bool,
    /// The right-click menu, while open, and where it opened.
    pub(super) menu: Option<(Entity<crate::ui::menu::Menu>, Point<Pixels>)>,
}

/// What a masked field draws for each character.
const DOT: char = '•';

impl EventEmitter<Submitted> for TextArea {}
impl EventEmitter<SubmittedBack> for TextArea {}
impl EventEmitter<Changed> for TextArea {}
impl EventEmitter<BackspaceEmpty> for TextArea {}
impl EventEmitter<Arrow> for TextArea {}
impl EventEmitter<Tabbed> for TextArea {}

impl TextArea {
    pub fn new(
        placeholder: impl Into<SharedString>,
        label: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
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
            text_color: gpui::white(),
            placeholder_color: gpui::white(),
            selection_color: gpui::white(),
            single_line: false,
            enter_newline: false,
            masked: false,
            menu_keys: false,
            menu: None,
        }
    }

    /// The text as drawn: the content, or a dot for each of its characters.
    pub(super) fn shown(&self) -> String {
        if self.masked {
            self.content.chars().map(|_| DOT).collect()
        } else {
            self.content.clone()
        }
    }

    /// Where content offset `offset` falls in the drawn text.
    pub(super) fn shown_offset(&self, offset: usize) -> usize {
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
        self.content = text.to_string();
        self.selected = text.len()..text.len();
        self.reversed = false;
        self.marked = None;
        cx.notify();
    }

    /// Wraps the selection in `before` and `after`, keeping it selected, as the format bar does.
    pub fn surround(&mut self, before: &str, after: &str, window: &Window, cx: &mut Context<Self>) {
        let range = self.selected.clone();
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

    /// Starts the cursor's line with `prefix`, as the list buttons do.
    pub fn prefix_line(&mut self, prefix: &str, window: &Window, cx: &mut Context<Self>) {
        let start = self.content[..self.cursor()]
            .rfind('\n')
            .map_or(0, |i| i + 1);
        self.content.insert_str(start, prefix);
        // Its range was measured before the text moved.
        self.marked = None;
        cx.emit(Changed);
        self.selected = self.selected.start + prefix.len()..self.selected.end + prefix.len();
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    /// Where the caret is, unless text is selected.
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
        self.content.replace_range(range.clone(), text);
        cx.emit(Changed);
        let end = range.start + text.len();
        self.selected = end..end;
        self.reversed = false;
        self.marked = None;
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    pub(super) fn cursor(&self) -> usize {
        if self.reversed {
            self.selected.start
        } else {
            self.selected.end
        }
    }

    pub(super) fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        self.selected = offset..offset;
        self.reversed = false;
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    pub(super) fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
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

    /// The word around `offset`, or the run of blanks or of punctuation it is in.
    pub(super) fn word_at(&self, offset: usize) -> Range<usize> {
        let kind = |c: char| {
            if c.is_alphanumeric() || c == '_' {
                0
            } else if c.is_whitespace() {
                1
            } else {
                2
            }
        };
        let (before, after) = self.content.split_at(offset.min(self.content.len()));
        // Between two runs, the press belongs to the one after it.
        let Some(here) = after.chars().next().or_else(|| before.chars().next_back()) else {
            return 0..0;
        };
        let here = kind(here);
        let start = before
            .char_indices()
            .rev()
            .take_while(|(_, c)| kind(*c) == here && *c != '\n')
            .last()
            .map_or(before.len(), |(i, _)| i);
        let end = after
            .char_indices()
            .find(|(_, c)| kind(*c) != here || *c == '\n')
            .map_or(self.content.len(), |(i, _)| before.len() + i);
        start..end
    }

    /// The line `offset` is on, without its line end.
    pub(super) fn line_at(&self, offset: usize) -> Range<usize> {
        let offset = offset.min(self.content.len());
        let start = self.content[..offset].rfind('\n').map_or(0, |i| i + 1);
        let end = self.content[offset..]
            .find('\n')
            .map_or(self.content.len(), |i| offset + i);
        start..end
    }

    pub(super) fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(i, _)| (i < offset).then_some(i))
            .unwrap_or(0)
    }

    pub(super) fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(i, _)| (i > offset).then_some(i))
            .unwrap_or(self.content.len())
    }

    /// Where `offset` was drawn, relative to the field.
    pub(super) fn position_for(&self, offset: usize) -> Option<Point<Pixels>> {
        let offset = self.shown_offset(offset);
        let line = self.lines.iter().rev().find(|line| line.start <= offset)?;
        let at = line
            .layout
            .position_for_index(offset - line.start, self.line_height)?;
        Some(point(at.x, at.y + line.top))
    }

    /// The text offset nearest `position`, relative to the field.
    pub(super) fn offset_for(&self, position: Point<Pixels>) -> usize {
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
