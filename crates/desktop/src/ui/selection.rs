//! Selecting text that is only read: a drag over the chat's text marks it, across paragraphs,
//! headings, table cells and code, and Ctrl+C copies what is marked. A double click marks a word,
//! a triple click the whole paragraph, and a right click opens a menu to copy it.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::ops::Range;

use gpui::{
    AnyElement, App, Bounds, ClipboardItem, CursorStyle, DispatchPhase, Element, ElementId, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, KeyBinding, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString,
    StyledText, TextLayout, Window, actions, fill, point, ContentMask, ListState, Styled, canvas,
    px,
};

use crate::ui::menu::{Dismissed, Menu, MenuItem};
use crate::ui::theme::theme;

actions!(selection, [CopySelection]);

/// The key that copies the marked text, where no text field has the keys.
pub fn bindings() -> Vec<KeyBinding> {
    vec![KeyBinding::new("ctrl-c", CopySelection, Some("Workspace"))]
}

/// One text as it was last drawn.
struct Drawn {
    id: u64,
    layout: TextLayout,
    prefix: Option<SharedString>,
    /// How far left of the text its prefix is drawn.
    gutter: Pixels,
    after_tab: bool,
}

/// What is marked, and the texts a drag can reach.
#[derive(Default)]
struct Selection {
    /// The texts drawn since the chat was last laid out, which is every one on screen.
    drawn: Vec<Drawn>,
    /// The marked part of each text, by the text's id.
    marked: HashMap<u64, Range<usize>>,
    /// The last text marked, top to bottom: the only one whose highlight stops where its mark does.
    last: Option<u64>,
    /// The marked text, top to bottom, as Ctrl+C copies it.
    text: String,
    /// The text the drag began in, and where in it.
    anchor: Option<(u64, usize)>,
    dragging: bool,
    /// Where the pointer was when the drag was last worked out: every text hears each move.
    seen: Option<Point<Pixels>>,
    /// The right-click menu, while open, and where it opened.
    menu: Option<(Entity<Menu>, Point<Pixels>)>,
}

impl gpui::Global for Selection {}

/// The right-click menu over the marked text, while it is open.
pub fn menu(cx: &mut App) -> Option<AnyElement> {
    let selection = cx.default_global::<Selection>();
    selection
        .menu
        .as_ref()
        .map(|(menu, at)| Menu::view(menu, *at))
}

/// Forgets the texts drawn before, as the chat is laid out again and draws the ones now on screen.
pub fn begin_frame(cx: &mut App) {
    cx.default_global::<Selection>().drawn.clear();
}

/// Copies the marked text, if any is.
pub fn copy(cx: &mut App) {
    let text = cx.default_global::<Selection>().text.clone();
    if !text.is_empty() {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }
}

/// A text that a drag can mark. `id` tells it from every other text on screen, and stays the
/// same from one frame to the next.
pub struct Selectable {
    id: u64,
    text: StyledText,
    prefix: Option<SharedString>,
    /// How far left of the text its prefix is drawn: a press there starts a drag too.
    gutter: Pixels,
    after_tab: bool,
}

/// Wraps `text` so a drag can mark it.
pub fn selectable(id: u64, text: StyledText) -> Selectable {
    Selectable {
        id,
        text,
        prefix: None,
        gutter: Pixels::ZERO,
        after_tab: false,
    }
}

impl Selectable {
    /// What is drawn beside the text rather than in it, such as a list item's bullet or number,
    /// copied before the text when it is marked from its start. It is drawn within `gutter` to the
    /// left of the text, and a drag can start on it.
    pub fn prefix(mut self, prefix: impl Into<SharedString>, gutter: Pixels) -> Self {
        self.prefix = Some(prefix.into());
        self.gutter = gutter;
        self
    }

    /// Copies a tab rather than a line break between the text before this one and it, as a
    /// table's cells after the first in their row.
    pub fn after_tab(mut self) -> Self {
        self.after_tab = true;
        self
    }
}

impl IntoElement for Selectable {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Selectable {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.text.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        self.text
            .prepaint(id, inspector_id, bounds, request_layout, window, cx);
        let mut bounds = bounds;
        bounds.origin.x -= self.gutter;
        bounds.size.width += self.gutter;
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.text.layout().clone();
        let selection = cx.default_global::<Selection>();
        let marked = selection.marked.get(&self.id).cloned();
        let continues = selection.last != Some(self.id);
        if let Some(range) = marked {
            let color = theme(cx).accent.opacity(0.3);
            // Marked on to its end with more marked below, the last row runs to the edge, as
            // the line end is marked too.
            let to_edge = continues && range.end >= layout.len();
            for quad in quads(&layout, &range, to_edge) {
                window.paint_quad(fill(quad, color));
            }
            // Marked from its start, its bullet or number is marked with it, as it is copied.
            if range.start == 0
                && self.gutter > Pixels::ZERO
                && let Some(start) = layout.position_for_index(0)
            {
                window.paint_quad(fill(
                    Bounds::from_corners(
                        point(start.x - self.gutter, start.y),
                        point(start.x, start.y + layout.line_height()),
                    ),
                    color,
                ));
            }
        }
        self.text.paint(
            id,
            inspector_id,
            bounds,
            request_layout,
            &mut (),
            window,
            cx,
        );
        window.set_cursor_style(CursorStyle::IBeam, prepaint);
        let selection = cx.default_global::<Selection>();
        selection.drawn.retain(|drawn| drawn.id != self.id);
        selection.drawn.push(Drawn {
            id: self.id,
            layout: layout.clone(),
            prefix: self.prefix.clone(),
            gutter: self.gutter,
            after_tab: self.after_tab,
        });

        let (id, hitbox) = (self.id, prepaint.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            let selection = cx.default_global::<Selection>();
            let hovered = phase == DispatchPhase::Bubble && hitbox.is_hovered(window);
            // A press on the prefix, left of the text, counts as one at the start of its line.
            let at = || {
                let x = event.position.x.max(layout.bounds().left());
                match layout.index_for_position(point(x, event.position.y)) {
                    Ok(at) | Err(at) => at,
                }
            };
            match event.button {
                // A press anywhere drops what was marked, unless it is on the open menu, which
                // keeps it to copy; the text under it then starts anew.
                MouseButton::Left if phase == DispatchPhase::Capture => {
                    if selection.menu.is_some() {
                        return;
                    }
                    selection.anchor = None;
                    selection.dragging = false;
                    if !selection.marked.is_empty() {
                        selection.marked.clear();
                        selection.text.clear();
                        window.refresh();
                    }
                }
                MouseButton::Left if hovered => {
                    let (at, text) = (at(), layout.text());
                    match event.click_count {
                        0 | 1 => {
                            selection.anchor = Some((id, at));
                            selection.dragging = true;
                            selection.seen = None;
                        }
                        2 => selection.mark(id, word_at(&text, at), &text),
                        _ => selection.mark(id, 0..text.len(), &text),
                    }
                    window.refresh();
                }
                // A right click keeps what is marked under it, or else marks the word there.
                MouseButton::Right if hovered => {
                    let (at, text) = (at(), layout.text());
                    let on_marked = selection
                        .marked
                        .get(&id)
                        .is_some_and(|range| range.start <= at && at <= range.end);
                    if !on_marked {
                        selection.anchor = None;
                        selection.dragging = false;
                        selection.mark(id, word_at(&text, at), &text);
                    }
                    open_menu(id, text, event.position, window, cx);
                }
                _ => {}
            }
        });
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            let selection = cx.default_global::<Selection>();
            if phase != DispatchPhase::Bubble
                || !selection.dragging
                || event.pressed_button != Some(MouseButton::Left)
                || selection.seen == Some(event.position)
            {
                return;
            }
            selection.seen = Some(event.position);
            selection.drag_to(event.position);
            window.refresh();
        });
        window.on_mouse_event(move |event: &MouseUpEvent, _, _, cx| {
            if event.button == MouseButton::Left {
                cx.default_global::<Selection>().dragging = false;
            }
        });
    }
}

/// Opens the menu for the text `id`, whose words are `text`, at `at`.
fn open_menu(id: u64, text: String, at: Point<Pixels>, window: &mut Window, cx: &mut App) {
    let marked = cx.default_global::<Selection>().text.clone();
    let mut items = Vec::new();
    if !marked.is_empty() {
        items.push(
            MenuItem::new("Copy", move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(marked.clone()));
            })
            .icon("copy"),
        );
    }
    items.push(MenuItem::new("Select paragraph", move |window, cx| {
        cx.default_global::<Selection>()
            .mark(id, 0..text.len(), &text);
        window.refresh();
    }));
    let back = window.focused(cx);
    let menu = Menu::open("Text", items, window, cx);
    window
        .subscribe(&menu, cx, move |_, _: &Dismissed, window, cx| {
            cx.default_global::<Selection>().menu = None;
            if let Some(back) = &back {
                window.focus(back, cx);
            }
            window.refresh();
        })
        .detach();
    cx.default_global::<Selection>().menu = Some((menu, at));
    window.refresh();
}

/// The word of `text` at byte `at`, or the one character there when it is not in a word.
fn word_at(text: &str, at: usize) -> Range<usize> {
    let at = at.min(text.len());
    if !text.is_char_boundary(at) {
        return at..at;
    }
    let in_word = |c: char| c.is_alphanumeric() || c == '_';
    let start = text[..at]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| in_word(c))
        .last()
        .map_or(at, |(i, _)| i);
    let end = text[at..]
        .char_indices()
        .find(|&(_, c)| !in_word(c))
        .map_or(text.len(), |(i, _)| at + i);
    if start < end {
        return start..end;
    }
    text[at..]
        .chars()
        .next()
        .map_or(at..at, |c| at..at + c.len_utf8())
}

impl Selection {
    /// Marks `range` of the one text `id`, whose words are `text`, and nothing else.
    fn mark(&mut self, id: u64, range: Range<usize>, text: &str) {
        self.marked.clear();
        self.text.clear();
        if range.start < range.end
            && range.end <= text.len()
            && text.is_char_boundary(range.start)
            && text.is_char_boundary(range.end)
        {
            self.text = text[range.clone()].to_string();
            self.marked.insert(id, range);
            self.last = Some(id);
        }
    }

    /// The space between two marked texts one above the other, such as the gap between
    /// paragraphs, so a mark across them reads as one. Wider gaps, which hold something that
    /// cannot be marked, stay clear.
    fn gaps(&self) -> Vec<Bounds<Pixels>> {
        let mut marked: Vec<&Drawn> = self
            .drawn
            .iter()
            .filter(|d| self.marked.contains_key(&d.id))
            .collect();
        marked.sort_by(|a, b| {
            let (a, b) = (a.layout.bounds(), b.layout.bounds());
            (a.top(), a.left())
                .partial_cmp(&(b.top(), b.left()))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        marked
            .windows(2)
            .filter_map(|pair| {
                let (a, b) = (pair[0], pair[1]);
                let (ab, bb) = (a.layout.bounds(), b.layout.bounds());
                let through = self.marked.get(&a.id)?.end >= a.layout.len()
                    && self.marked.get(&b.id)?.start == 0;
                let gap = bb.top() - ab.bottom();
                (through && gap > Pixels::ZERO && gap <= a.layout.line_height() * 2.).then(|| {
                    Bounds::from_corners(
                        point(
                            (ab.left() - a.gutter).min(bb.left() - b.gutter),
                            ab.bottom(),
                        ),
                        point(ab.right().max(bb.right()), bb.top()),
                    )
                })
            })
            .collect()
    }

    /// Marks everything from where the drag began to `position`, text by text, top to bottom.
    fn drag_to(&mut self, position: Point<Pixels>) {
        let Some((anchor, anchor_at)) = self.anchor else {
            return;
        };
        self.drawn.sort_by(|a, b| {
            let (a, b) = (a.layout.bounds(), b.layout.bounds());
            (a.top(), a.left())
                .partial_cmp(&(b.top(), b.left()))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let Some(from) = self.drawn.iter().position(|drawn| drawn.id == anchor) else {
            return;
        };
        // The text under the pointer, or else the last one that starts above it.
        let to = self
            .drawn
            .iter()
            .position(|drawn| drawn.layout.bounds().contains(&position))
            .or_else(|| {
                self.drawn
                    .iter()
                    .rposition(|drawn| drawn.layout.bounds().top() <= position.y)
            });
        let (to, to_at) = match to {
            Some(to) => {
                let layout = &self.drawn[to].layout;
                let bounds = layout.bounds();
                let at = if position.y >= bounds.bottom() {
                    layout.len()
                } else {
                    // Beside the text, the nearest place on the pointer's line.
                    let x = position
                        .x
                        .max(bounds.left())
                        .min(bounds.right() - gpui::px(1.));
                    match layout.index_for_position(point(x, position.y)) {
                        Ok(at) | Err(at) => at,
                    }
                };
                (to, at)
            }
            // Above every text.
            None => (0, 0),
        };
        let ((first, first_at), (last, last_at)) = if (from, anchor_at) <= (to, to_at) {
            ((from, anchor_at), (to, to_at))
        } else {
            ((to, to_at), (from, anchor_at))
        };
        self.marked.clear();
        self.text.clear();
        for (i, drawn) in self.drawn.iter().enumerate().take(last + 1).skip(first) {
            let text = drawn.layout.text();
            let start = if i == first { first_at } else { 0 };
            let end = if i == last { last_at } else { text.len() };
            let (start, end) = (start.min(text.len()), end.min(text.len()));
            if start < end && text.is_char_boundary(start) && text.is_char_boundary(end) {
                if !self.marked.is_empty() {
                    self.text.push(if drawn.after_tab { '\t' } else { '\n' });
                }
                if start == 0
                    && let Some(prefix) = &drawn.prefix
                {
                    self.text.push_str(prefix);
                }
                self.text.push_str(&text[start..end]);
                self.marked.insert(drawn.id, start..end);
                self.last = Some(drawn.id);
            }
        }
    }
}

/// The boxes behind `range` of a text: one for a single line, or the end of the first line, the
/// lines between, and the start of the last.
fn quads(layout: &TextLayout, range: &Range<usize>, to_edge: bool) -> Vec<Bounds<Pixels>> {
    let (Some(start), Some(mut end)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return Vec::new();
    };
    let (bounds, line) = (layout.bounds(), layout.line_height());
    if to_edge {
        end.x = bounds.right();
    }
    if start.y == end.y {
        return vec![Bounds::from_corners(start, point(end.x, end.y + line))];
    }
    let mut quads = vec![
        Bounds::from_corners(start, point(bounds.right(), start.y + line)),
        Bounds::from_corners(point(bounds.left(), end.y), point(end.x, end.y + line)),
    ];
    if start.y + line < end.y {
        quads.push(Bounds::from_corners(
            point(bounds.left(), start.y + line),
            point(bounds.right(), end.y),
        ));
    }
    quads
}

/// Drawn over a scrolling list of texts, after them: it fills the gaps between the marked ones,
/// and while a drag holds the pointer above or below the list, it scrolls the list that way and
/// marks on.
pub fn overlay(list: ListState) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, cx| {
            let color = theme(cx).accent.opacity(0.3);
            let selection = cx.default_global::<Selection>();
            if selection.dragging
                && let Some(at) = selection.seen
            {
                let past = if at.y < bounds.top() {
                    at.y - bounds.top()
                } else if at.y > bounds.bottom() {
                    at.y - bounds.bottom()
                } else {
                    Pixels::ZERO
                };
                if past != Pixels::ZERO {
                    // The further out the pointer, the faster it goes.
                    list.scroll_by((past / 2.).clamp(px(-40.), px(40.)));
                    selection.drag_to(at);
                    window.request_animation_frame();
                }
            }
            let gaps = selection.gaps();
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                for gap in gaps {
                    window.paint_quad(fill(gap, color));
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// An id for the `nth` text drawn under `key`.
pub fn text_id(key: &SharedString, nth: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (key, nth).hash(&mut hasher);
    hasher.finish()
}
