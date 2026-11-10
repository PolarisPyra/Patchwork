//! A multi-line plain-text field: the composer's editor. It takes text from the platform's input
//! method, so typing, pasting and composing in other scripts work, and wraps lines to its width.

// Rust guideline compliant 2026-10-09

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, Hsla, KeyBinding,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    Role, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine,
    actions, div, fill, point, prelude::*, px, relative, size,
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
struct DrawnLine {
    start: usize,
    layout: WrappedLine,
    top: Pixels,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate piece of view state"
)]
pub struct TextArea {
    focus: FocusHandle,
    pub content: String,
    pub placeholder: SharedString,
    label: SharedString,
    selected: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    lines: Vec<DrawnLine>,
    bounds: Option<Bounds<Pixels>>,
    line_height: Pixels,
    selecting: bool,
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
        }
    }

    /// The text as drawn: the content, or a dot for each of its characters.
    fn shown(&self) -> String {
        if self.masked {
            self.content.chars().map(|_| DOT).collect()
        } else {
            self.content.clone()
        }
    }

    /// Where content offset `offset` falls in the drawn text.
    fn shown_offset(&self, offset: usize) -> usize {
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

    fn cursor(&self) -> usize {
        if self.reversed {
            self.selected.start
        } else {
            self.selected.end
        }
    }

    fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        self.selected = offset..offset;
        self.reversed = false;
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
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

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(i, _)| (i < offset).then_some(i))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(i, _)| (i > offset).then_some(i))
            .unwrap_or(self.content.len())
    }

    /// Where `offset` was drawn, relative to the field.
    fn position_for(&self, offset: usize) -> Option<Point<Pixels>> {
        let offset = self.shown_offset(offset);
        let line = self.lines.iter().rev().find(|line| line.start <= offset)?;
        let at = line
            .layout
            .position_for_index(offset - line.start, self.line_height)?;
        Some(point(at.x, at.y + line.top))
    }

    /// The text offset nearest `position`, relative to the field.
    fn offset_for(&self, position: Point<Pixels>) -> usize {
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

    fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        let to = if self.selected.is_empty() {
            self.previous_boundary(self.cursor())
        } else {
            self.selected.start
        };
        self.move_to(to, window, cx);
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        let to = if self.selected.is_empty() {
            self.next_boundary(self.selected.end)
        } else {
            self.selected.end
        };
        self.move_to(to, window, cx);
    }

    fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        if self.single_line || self.menu_keys {
            return cx.emit(Arrow(-1));
        }
        let to = self.vertical(-1.).unwrap_or(0);
        self.move_to(to, window, cx);
    }

    fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        if self.single_line || self.menu_keys {
            return cx.emit(Arrow(1));
        }
        let to = self.vertical(1.).unwrap_or(self.content.len());
        self.move_to(to, window, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor()), window, cx);
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor()), window, cx);
    }

    fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        let to = self.vertical(-1.).unwrap_or(0);
        self.select_to(to, window, cx);
    }

    fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        let to = self.vertical(1.).unwrap_or(self.content.len());
        self.select_to(to, window, cx);
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.select_everything(window, cx);
    }

    /// Selects the whole text, so typing replaces it.
    pub fn select_everything(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
        self.select_to(self.content.len(), window, cx);
    }

    /// The start of the cursor's line.
    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        let start = self.content[..self.cursor()]
            .rfind('\n')
            .map_or(0, |i| i + 1);
        self.move_to(start, window, cx);
    }

    /// The end of the cursor's line.
    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.cursor();
        let end = self.content[cursor..]
            .find('\n')
            .map_or(self.content.len(), |i| cursor + i);
        self.move_to(end, window, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.content.is_empty() {
            return cx.emit(BackspaceEmpty);
        }
        if self.selected.is_empty() {
            self.select_to(self.previous_boundary(self.cursor()), window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.select_to(self.next_boundary(self.cursor()), window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.single_line {
            return cx.emit(SubmittedBack);
        }
        self.replace_text_in_range(None, "\n", window, cx);
    }

    fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        if self.enter_newline {
            return self.replace_text_in_range(None, "\n", window, cx);
        }
        cx.emit(Submitted);
    }

    fn ctrl_submit(&mut self, _: &CtrlSubmit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(Submitted);
    }

    /// Tab picks from an open menu; otherwise it moves the focus on.
    fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        if self.menu_keys {
            cx.emit(Tabbed);
        } else {
            cx.propagate();
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selecting = true;
        let offset = self.offset_at(event.position);
        if event.modifiers.shift {
            self.select_to(offset, window, cx);
        } else {
            self.move_to(offset, window, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn on_mouse_move(
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

    fn offset_to_utf16(&self, offset: usize) -> usize {
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

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range);
        adjusted_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected),
            reversed: self.reversed,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = self.range_from_utf16(&range_utf16);
        self.reversed = false;
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.content.encode_utf16().count())
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        self.content.replace_range(range.clone(), text);
        cx.emit(Changed);
        let end = range.start + text.len();
        self.selected = end..end;
        self.reversed = false;
        self.marked = None;
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        self.content.replace_range(range.clone(), new_text);
        cx.emit(Changed);
        self.marked = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        let start16 = self.offset_to_utf16(range.start);
        let len16 = new_text.encode_utf16().count();
        self.selected = new_selected_range.map_or_else(
            || range.start + new_text.len()..range.start + new_text.len(),
            |s| self.range_from_utf16(&(start16 + s.start.min(len16)..start16 + s.end.min(len16))),
        );
        self.reversed = false;
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let start = self.position_for(range.start)?;
        let end = self.position_for(range.end)?;
        Some(Bounds::from_corners(
            element_bounds.origin + start,
            element_bounds.origin + point(end.x, end.y + self.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.bounds?;
        Some(self.offset_to_utf16(self.offset_for(point - bounds.origin)))
    }
}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TextArea {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("text-area")
            .role(if self.single_line {
                Role::TextInput
            } else {
                Role::MultilineTextInput
            })
            .aria_label(self.label.clone())
            .aria_placeholder(self.placeholder.clone())
            .key_context("TextArea")
            .track_focus(&self.focus)
            .tab_index(0)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::ctrl_submit))
            .on_action(cx.listener(Self::tab))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .child(TextElement { area: cx.entity() })
    }
}

/// Lays the text out at the field's width and draws it with its selection and caret.
struct TextElement {
    area: Entity<TextArea>,
}

struct Prepainted {
    lines: Vec<DrawnLine>,
    quads: Vec<PaintQuad>,
    caret: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Shapes `text` one `\n`-line at a time, wrapped to `width`.
fn shape(
    text: &str,
    color: Hsla,
    width: Pixels,
    line_height: Pixels,
    marked: Option<&Range<usize>>,
    window: &mut Window,
) -> Vec<DrawnLine> {
    let style = window.text_style();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let mut lines = Vec::new();
    let mut start = 0;
    let mut top = px(0.);
    for line in text.split('\n') {
        let end = start + line.len();
        let run = TextRun {
            len: line.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match marked {
            Some(m) if m.start < end && m.end > start => {
                let (a, b) = (m.start.max(start) - start, m.end.min(end) - start);
                vec![
                    TextRun {
                        len: a,
                        ..run.clone()
                    },
                    TextRun {
                        len: b - a,
                        underline: Some(UnderlineStyle {
                            color: Some(color),
                            thickness: px(1.),
                            wavy: false,
                        }),
                        ..run.clone()
                    },
                    TextRun {
                        len: line.len() - b,
                        ..run
                    },
                ]
            }
            _ => vec![run],
        };
        let shaped = window
            .text_system()
            .shape_text(
                SharedString::from(line.to_string()),
                font_size,
                &runs,
                Some(width),
                None,
            )
            .ok()
            .and_then(|mut v| (!v.is_empty()).then(|| v.remove(0)));
        if let Some(layout) = shaped {
            let height = layout.size(line_height).height.max(line_height);
            lines.push(DrawnLine { start, layout, top });
            top += height;
        }
        start = end + 1;
    }
    lines
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let area = self.area.read(cx);
        let text = if area.content.is_empty() {
            area.placeholder.to_string()
        } else {
            area.shown()
        };
        let line_height = window.line_height();
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let layout = window.request_measured_layout(style, move |known, available, window, _| {
            let width = known.width.unwrap_or(match available.width {
                gpui::AvailableSpace::Definite(w) => w,
                _ => px(10_000.),
            });
            let lines = shape(&text, gpui::white(), width, line_height, None, window);
            let height = lines.last().map_or(line_height, |l| {
                l.top + l.layout.size(line_height).height.max(line_height)
            });
            size(width, height)
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let area = self.area.read(cx);
        let line_height = window.line_height();
        let empty = area.content.is_empty();
        let (text, color) = if empty {
            (area.placeholder.to_string(), area.placeholder_color)
        } else {
            (area.shown(), area.text_color)
        };
        let marked = area
            .marked
            .clone()
            .map(|m| area.shown_offset(m.start)..area.shown_offset(m.end));
        let lines = shape(
            &text,
            color,
            bounds.size.width,
            line_height,
            marked.as_ref(),
            window,
        );
        let selection_color = area.selection_color;
        let caret_color = area.text_color;
        let selected = if empty {
            0..0
        } else {
            area.shown_offset(area.selected.start)..area.shown_offset(area.selected.end)
        };
        let cursor = if empty {
            0
        } else {
            area.shown_offset(area.cursor())
        };
        // Positions are found against these lines, not the ones from the last frame.
        let at = |offset: usize| -> Option<Point<Pixels>> {
            let line = lines.iter().rev().find(|l| l.start <= offset)?;
            let p = line
                .layout
                .position_for_index(offset - line.start, line_height)?;
            Some(point(p.x, p.y + line.top))
        };
        let mut quads = Vec::new();
        if !selected.is_empty()
            && let (Some(a), Some(b)) = (at(selected.start), at(selected.end))
        {
            let origin = bounds.origin;
            let right = bounds.size.width;
            let mut row = |x0: Pixels, x1: Pixels, y: Pixels| {
                quads.push(fill(
                    Bounds::from_corners(
                        origin + point(x0, y),
                        origin + point(x1.max(x0 + px(4.)), y + line_height),
                    ),
                    selection_color,
                ));
            };
            if a.y == b.y {
                row(a.x, b.x, a.y);
            } else {
                row(a.x, right, a.y);
                let mut y = a.y + line_height;
                while y < b.y {
                    row(px(0.), right, y);
                    y += line_height;
                }
                row(px(0.), b.x, b.y);
            }
        }
        let caret = selected.is_empty().then(|| at(cursor)).flatten().map(|p| {
            fill(
                Bounds::new(bounds.origin + p, size(px(1.), line_height)),
                caret_color,
            )
        });
        Prepainted {
            lines,
            quads,
            caret,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.area.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.area.clone()),
            cx,
        );
        for quad in prepaint.quads.drain(..) {
            window.paint_quad(quad);
        }
        let line_height = window.line_height();
        for line in &prepaint.lines {
            if let Err(e) = line.layout.paint(
                bounds.origin + point(px(0.), line.top),
                line_height,
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            ) {
                log::warn!(error:% = e; "could not draw the composer's text: {{error}}");
            }
        }
        if focus.is_focused(window)
            && let Some(caret) = prepaint.caret.take()
        {
            window.paint_quad(caret);
        }
        let lines = std::mem::take(&mut prepaint.lines);
        self.area.update(cx, |area, _| {
            area.lines = lines;
            area.bounds = Some(bounds);
            area.line_height = line_height;
        });
    }
}
