//! Selecting text that is only read: a drag over the chat's text marks it, across paragraphs,
//! headings, table cells and code, and Ctrl+C copies what is marked. A double click marks a word,
//! a triple click the whole paragraph, and a right click opens a menu to copy it.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, ClipboardItem, CursorStyle, DispatchPhase, Element, ElementId, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, KeyBinding,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    SharedString, StyledText, TextLayout, Window, actions, fill, point, ContentMask, ListState,
    Styled, canvas, px,
};

use crate::ui::menu::{Dismissed, Menu, MenuItem};
use crate::ui::theme::{radius_sm, theme};

/// How far a chip reaches past its first and last letter, and how far in from its line's top and
/// bottom it stops, so chips on lines above each other keep apart.
const CHIP_PAD_X: f32 = 3.;
const CHIP_INSET_Y: f32 = 1.;

actions!(selection, [CopySelection]);

/// The key that copies the marked text, where no text field has the keys.
pub fn bindings() -> Vec<KeyBinding> {
    vec![KeyBinding::new("ctrl-c", CopySelection, Some("Workspace"))]
}

/// One text drawn this frame, where a pointer can reach it.
struct Drawn {
    id: u64,
    layout: TextLayout,
    /// How far left of the text its prefix is drawn.
    gutter: Pixels,
    /// How far right of the text a mark that runs on past its end reaches.
    reach: Pixels,
    /// The part of the text not clipped by the list it scrolls in.
    visible: Bounds<Pixels>,
}

/// Where a text sits in reading order: the list row it is in, then how far down and across that
/// row. A text outside a list uses row 0 and its place on screen.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct Place {
    row: usize,
    y: f32,
    x: f32,
}

/// What block a text is. A drag over more than one text copies them as Markdown, so pasting
/// into a message keeps the paragraphs, lists, headings, code and tables apart.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Kind {
    #[default]
    Paragraph,
    /// A list item's text, whose prefix is its `- ` or `3. `.
    Item,
    /// A heading, by level from 1.
    Heading(usize),
    /// A code block, by its language.
    Code(SharedString),
    /// A table cell, by its column from 0, and whether its row is the header.
    Cell { column: usize, head: bool },
}

/// A styled range of a text, with the Markdown that opened and closed it, such as `**` and `**`
/// or `[` and `](https://…)`.
pub type Mark = (Range<usize>, SharedString, SharedString);

/// A text seen since the selection began, kept after it scrolls away so a mark can run past it.
struct Known {
    place: Place,
    layout: TextLayout,
    prefix: Option<SharedString>,
    kind: Kind,
    lead: SharedString,
    marks: Vec<Mark>,
}

/// What is marked, and the texts a drag can reach.
#[derive(Default)]
struct Selection {
    /// The texts drawn since the chat was last laid out, which is every one on screen.
    drawn: Vec<Drawn>,
    /// Every text seen since the selection began, by id, on screen or not.
    known: HashMap<u64, Known>,
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
    /// How far right of the text a mark that runs on past its end reaches, and a press counts.
    reach: Pixels,
    kind: Kind,
    /// What each copied line of the text starts with, for the quotes and list items it is in.
    lead: SharedString,
    /// The Markdown that was around each styled range, put back when the range is copied.
    marks: Vec<Mark>,
    /// Ranges drawn as rounded chips behind the text, each with its colour.
    chips: Vec<(Range<usize>, Hsla)>,
    /// Ranges a click opens, each with its address.
    links: Vec<(Range<usize>, SharedString)>,
}

/// Wraps `text` so a drag can mark it.
pub fn selectable(id: u64, text: StyledText) -> Selectable {
    Selectable {
        id,
        text,
        prefix: None,
        gutter: Pixels::ZERO,
        reach: Pixels::ZERO,
        kind: Kind::Paragraph,
        lead: SharedString::default(),
        marks: Vec::new(),
        chips: Vec::new(),
        links: Vec::new(),
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

    /// Lets a mark reach `pad` past both sides of the text, as across a table cell's padding
    /// to its border, so the marks of cells side by side meet.
    pub fn pad(mut self, pad: Pixels) -> Self {
        self.gutter = pad;
        self.reach = pad;
        self
    }

    /// What block the text is, which sets the Markdown around it when it is copied with others.
    pub fn kind(mut self, kind: Kind) -> Self {
        self.kind = kind;
        self
    }

    /// Starts each line copied from the text with `lead`, as `> ` does in a quote.
    pub fn lead(mut self, lead: impl Into<SharedString>) -> Self {
        self.lead = lead.into();
        self
    }

    /// Puts back the Markdown each range was written with when it is copied.
    pub fn marks(mut self, marks: Vec<Mark>) -> Self {
        self.marks = marks;
        self
    }

    /// Draws each range as a rounded chip in its colour, padded a little past its letters.
    pub fn chips(mut self, chips: Vec<(Range<usize>, Hsla)>) -> Self {
        self.chips = chips;
        self
    }

    /// Opens each range's address in the browser when it is clicked.
    pub fn links(mut self, links: Vec<(Range<usize>, SharedString)>) -> Self {
        self.links = links;
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
        bounds.size.width += self.gutter + self.reach;
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
        for (range, color) in &self.chips {
            let range = range.start.min(layout.len())..range.end.min(layout.len());
            for quad in chip_quads(&layout, &range) {
                let quad = Bounds::from_corners(
                    point(quad.left() - px(CHIP_PAD_X), quad.top() + px(CHIP_INSET_Y)),
                    point(
                        quad.right() + px(CHIP_PAD_X),
                        quad.bottom() - px(CHIP_INSET_Y),
                    ),
                );
                window.paint_quad(fill(quad, *color).corner_radii(px(radius_sm())));
            }
        }
        let selection = cx.default_global::<Selection>();
        let marked = selection.marked.get(&self.id).cloned();
        let continues = selection.last != Some(self.id);
        if let Some(range) = marked {
            let color = theme(cx).accent.opacity(0.3);
            // Marked on to its end with more marked below, the last row runs to the edge, as
            // the line end is marked too.
            let to_edge = continues && range.end >= layout.len();
            let right = layout.bounds().right();
            for mut quad in quads(&layout, &range, to_edge) {
                if quad.right() >= right {
                    quad.size.width += self.reach;
                }
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
        let links = Rc::new(std::mem::take(&mut self.links));
        let hovered_link = link_under(&layout, &links, prepaint, window);
        let cursor = if hovered_link.is_some() {
            CursorStyle::PointingHand
        } else {
            CursorStyle::IBeam
        };
        window.set_cursor_style(cursor, prepaint);
        let selection = cx.default_global::<Selection>();
        let text_bounds = layout.bounds();
        selection.drawn.retain(|drawn| drawn.id != self.id);
        selection.drawn.push(Drawn {
            id: self.id,
            layout: layout.clone(),
            gutter: self.gutter,
            reach: self.reach,
            visible: text_bounds.intersect(&window.content_mask().bounds),
        });
        // Its place on screen, until the list it is in, if any, gives its row.
        selection.known.insert(
            self.id,
            Known {
                place: Place {
                    row: 0,
                    y: text_bounds.top().into(),
                    x: text_bounds.left().into(),
                },
                layout: layout.clone(),
                prefix: self.prefix.clone(),
                kind: self.kind.clone(),
                lead: self.lead.clone(),
                marks: self.marks.clone(),
            },
        );

        let (id, hitbox) = (self.id, prepaint.clone());
        let (move_layout, move_links, move_hitbox) =
            (layout.clone(), Rc::clone(&links), prepaint.clone());
        let (up_layout, up_links, up_hitbox) =
            (layout.clone(), Rc::clone(&links), prepaint.clone());
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
                    // A new selection starts from the texts on screen.
                    let Selection { drawn, known, .. } = &mut *selection;
                    known.retain(|id, _| drawn.iter().any(|drawn| drawn.id == *id));
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
                    let link =
                        link_at(&layout, &links, event.position).map(|link| links[link].1.clone());
                    open_menu(link, event.position, window, cx);
                }
                _ => {}
            }
        });
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            // The pointer moving onto or off a link changes the cursor.
            if phase == DispatchPhase::Bubble
                && !move_links.is_empty()
                && link_under(&move_layout, &move_links, &move_hitbox, window) != hovered_link
            {
                window.refresh();
            }
            // The drag follows the pointer in the capture phase, so nothing drawn over the text
            // can stop it.
            let selection = cx.default_global::<Selection>();
            if phase != DispatchPhase::Capture
                || !selection.dragging
                || selection.seen == Some(event.position)
            {
                return;
            }
            // The button was let go where the window did not hear it.
            if event.pressed_button != Some(MouseButton::Left) {
                selection.dragging = false;
                return;
            }
            selection.seen = Some(event.position);
            if selection.drag_to(event.position) {
                window.refresh();
            }
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if event.button != MouseButton::Left {
                return;
            }
            let selection = cx.default_global::<Selection>();
            selection.dragging = false;
            // A click that pressed and let go on the same link, marking nothing, opens it.
            if phase == DispatchPhase::Bubble
                && selection.marked.is_empty()
                && let Some((anchor, from)) = selection.anchor
                && anchor == id
                && up_hitbox.is_hovered(window)
                && let Some(link) = link_at(&up_layout, &up_links, event.position)
                && up_links[link].0.contains(&from)
            {
                cx.open_url(&up_links[link].1);
            }
        });
    }
}

/// Which of `links` is under the pointer, when it is over the text.
fn link_under(
    layout: &TextLayout,
    links: &[(Range<usize>, SharedString)],
    hitbox: &Hitbox,
    window: &Window,
) -> Option<usize> {
    if links.is_empty() || !hitbox.is_hovered(window) {
        return None;
    }
    link_at(layout, links, window.mouse_position())
}

/// Which of `links` holds the letter at `position`.
fn link_at(
    layout: &TextLayout,
    links: &[(Range<usize>, SharedString)],
    position: Point<Pixels>,
) -> Option<usize> {
    let at = layout.index_for_position(position).ok()?;
    links.iter().position(|(range, _)| range.contains(&at))
}

/// Opens the menu for a text at `at`, with the link under the pointer, if any.
fn open_menu(link: Option<SharedString>, at: Point<Pixels>, window: &mut Window, cx: &mut App) {
    let marked = cx.default_global::<Selection>().text.clone();
    let mut items = Vec::new();
    if let Some(link) = link {
        let copy = link.clone();
        items.push(MenuItem::new("Open link", move |_, cx| cx.open_url(&link)));
        items.push(
            MenuItem::new("Copy link", move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy.to_string()));
            })
            .icon("copy"),
        );
    }
    if !marked.is_empty() {
        items.push(
            MenuItem::new("Copy", move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(marked.clone()));
            })
            .icon("copy"),
        );
    }
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
            self.text = match self.known.get(&id) {
                Some(known) => markdown(&[(known, range.clone())]),
                None => text[range.clone()].to_string(),
            };
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
                        point((ab.right() + a.reach).max(bb.right() + b.reach), bb.top()),
                    )
                })
            })
            .collect()
    }

    /// Gives each text drawn in `list` its row there, so its place holds as the list scrolls.
    fn place_rows(&mut self, list: &ListState) {
        let Some(lowest) = self
            .drawn
            .iter()
            .map(|drawn| drawn.layout.bounds().top())
            .reduce(Pixels::max)
        else {
            return;
        };
        let mut rows = Vec::new();
        for row in list.logical_scroll_top().item_ix..list.item_count() {
            let Some(bounds) = list.bounds_for_item(row) else {
                break;
            };
            if bounds.top() > lowest {
                break;
            }
            rows.push((row, bounds));
        }
        for drawn in &self.drawn {
            let text = drawn.layout.bounds();
            if let Some(&(row, bounds)) = rows
                .iter()
                .find(|(_, bounds)| bounds.top() <= text.top() && text.top() < bounds.bottom())
                && let Some(known) = self.known.get_mut(&drawn.id)
            {
                known.place = Place {
                    row,
                    y: (text.top() - bounds.top()).into(),
                    x: text.left().into(),
                };
            }
        }
    }

    /// The text at `position` and the place in it, among the texts in view: the one under the
    /// pointer, or else the last one that starts above it, or else the first.
    fn hit(&self, position: Point<Pixels>) -> Option<(u64, usize)> {
        let mut shown: Vec<&Drawn> = self
            .drawn
            .iter()
            .filter(|drawn| !drawn.visible.is_empty())
            .collect();
        shown.sort_by(|a, b| {
            (a.visible.top(), a.visible.left())
                .partial_cmp(&(b.visible.top(), b.visible.left()))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let drawn = shown
            .iter()
            .find(|drawn| drawn.visible.contains(&position))
            .or_else(|| {
                shown
                    .iter()
                    .rev()
                    .find(|drawn| drawn.visible.top() <= position.y)
            })
            .or_else(|| shown.first())?;
        let (layout, bounds, visible) = (&drawn.layout, drawn.layout.bounds(), drawn.visible);
        // Past the end of a text shown whole, its end; before its start, its start. Past the part
        // of a text the list cuts off, the nearest line still in view.
        let at = if position.y >= visible.bottom() && visible.bottom() >= bounds.bottom() {
            layout.len()
        } else if position.y < visible.top() && visible.top() <= bounds.top() {
            0
        } else {
            let y = position.y.max(visible.top()).min(visible.bottom() - px(1.));
            let x = position.x.max(bounds.left()).min(bounds.right() - px(1.));
            match layout.index_for_position(point(x, y)) {
                Ok(at) | Err(at) => at,
            }
        };
        Some((drawn.id, at))
    }

    /// Marks everything from where the drag began to `position`, text by text in reading order,
    /// including texts scrolled out of view. Returns whether what is marked changed.
    fn drag_to(&mut self, position: Point<Pixels>) -> bool {
        let Some((anchor, anchor_at)) = self.anchor else {
            return false;
        };
        let Some((to, to_at)) = self.hit(position) else {
            return false;
        };
        let (Some(from_place), Some(to_place)) = (
            self.known.get(&anchor).map(|known| known.place),
            self.known.get(&to).map(|known| known.place),
        ) else {
            return false;
        };
        let ((first, first_place, first_at), (last, last_place, last_at)) =
            if (from_place, anchor_at) <= (to_place, to_at) {
                ((anchor, from_place, anchor_at), (to, to_place, to_at))
            } else {
                ((to, to_place, to_at), (anchor, from_place, anchor_at))
            };
        let mut between: Vec<(Place, u64)> = self
            .known
            .iter()
            .filter(|(_, known)| first_place <= known.place && known.place <= last_place)
            .map(|(id, known)| (known.place, *id))
            .collect();
        between.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let before = std::mem::take(&mut self.marked);
        let mut pieces = Vec::new();
        for (_, id) in between {
            let Some(known) = self.known.get(&id) else {
                continue;
            };
            let text = known.layout.text();
            let start = if id == first { first_at } else { 0 };
            let end = if id == last { last_at } else { text.len() };
            let (start, end) = (start.min(text.len()), end.min(text.len()));
            if start < end && text.is_char_boundary(start) && text.is_char_boundary(end) {
                pieces.push((known, start..end));
                self.marked.insert(id, start..end);
                self.last = Some(id);
            }
        }
        self.text = markdown(&pieces);
        self.marked != before
    }
}

/// The marked part of each text, top to bottom, as Ctrl+C copies it, written back as Markdown so
/// a paste into a message draws it as it was drawn here. Part of one text keeps only its inline
/// marks, and none that wraps all of it, so a bold word or a line of code copies as it reads.
fn markdown(pieces: &[(&Known, Range<usize>)]) -> String {
    if let [(known, range)] = pieces {
        let text = known.layout.text();
        if matches!(known.kind, Kind::Code(_)) {
            return text[range.clone()].to_string();
        }
        return marked(&text, range, &known.marks, true);
    }
    let mut out = String::new();
    let mut before: Option<&Known> = None;
    // The cells copied in the open table row, for the line under a header row.
    let mut cells = 0;
    for (known, range) in pieces {
        let text = known.layout.text();
        let (kind, lead) = (&known.kind, known.lead.as_ref());
        let next_cell = matches!(kind, Kind::Cell { column, .. } if *column > 0)
            && matches!(before.map(|b| &b.kind), Some(Kind::Cell { .. }));
        if !next_cell {
            end_row(&mut out, before, cells);
            let line = match &known.prefix {
                Some(prefix) if range.start == 0 => prefix.as_ref(),
                _ => lead,
            };
            match before {
                // The first line keeps no indent, which would read as code.
                None => out.push_str(line.trim_start()),
                Some(b) => {
                    let tight = matches!(
                        (&b.kind, kind),
                        (Kind::Item, Kind::Item) | (Kind::Cell { .. }, Kind::Cell { .. })
                    );
                    if !tight {
                        out.push('\n');
                        out.push_str(shared(&b.lead, lead).trim_end());
                    }
                    out.push('\n');
                    out.push_str(line);
                }
            }
        }
        match kind {
            Kind::Cell { .. } => {
                if next_cell {
                    out.push_str(" | ");
                } else {
                    out.push_str("| ");
                    cells = 0;
                }
                cells += 1;
                let cell = marked(&text, range, &known.marks, false);
                out.push_str(&cell.replace('|', "\\|").replace('\n', " "));
            }
            Kind::Code(lang) => {
                let code = &text[range.clone()];
                let mut fence = "```".to_string();
                while code.contains(fence.as_str()) {
                    fence.push('`');
                }
                out.push_str(&fence);
                out.push_str(lang);
                for line in code.split('\n') {
                    out.push('\n');
                    out.push_str(lead);
                    out.push_str(line);
                }
                out.push('\n');
                out.push_str(lead);
                out.push_str(&fence);
            }
            Kind::Heading(level) => {
                if range.start == 0 {
                    out.push_str(&"#".repeat(*level));
                    out.push(' ');
                }
                out.push_str(&marked(&text, range, &known.marks, false).replace('\n', " "));
            }
            Kind::Paragraph | Kind::Item => {
                // Two spaces keep a line break inside a paragraph, which a bare newline joins up.
                let lines = marked(&text, range, &known.marks, false);
                out.push_str(&lines.replace('\n', &format!("  \n{lead}")));
            }
        }
        before = Some(known);
    }
    end_row(&mut out, before, cells);
    out
}

/// `range` of `text` with each mark's Markdown put back around the part of it inside the range.
/// With `inside`, a mark that wraps the whole range is left out.
fn marked(text: &str, range: &Range<usize>, marks: &[Mark], inside: bool) -> String {
    // Where each opener and closer goes: closers before openers at one place, and of several
    // openers the one reaching furthest first, so the marks nest as they did.
    let mut edits: Vec<(usize, bool, usize, usize, &str)> = Vec::new();
    for (i, (span, open, close)) in marks.iter().enumerate() {
        let (mut start, mut end) = (span.start.max(range.start), span.end.min(range.end));
        // `**` beside a space does not open or close, so the marker moves in past it.
        while let Some(c) = text.get(start..end).and_then(|s| s.chars().next())
            && c.is_whitespace()
        {
            start += c.len_utf8();
        }
        while let Some(c) = text.get(start..end).and_then(|s| s.chars().next_back())
            && c.is_whitespace()
        {
            end -= c.len_utf8();
        }
        if start >= end || (inside && start == range.start && end == range.end) {
            continue;
        }
        edits.push((start, true, usize::MAX - end, i, open));
        edits.push((end, false, usize::MAX - start, usize::MAX - i, close));
    }
    edits.sort_by_key(|&(at, opens, reach, order, _)| (at, opens, reach, order));
    let mut out = String::new();
    let mut from = range.start;
    for (at, _, _, _, piece) in edits {
        out.push_str(text.get(from..at).unwrap_or(""));
        out.push_str(piece);
        from = at;
    }
    out.push_str(text.get(from..range.end).unwrap_or(""));
    out
}

/// The start two leads share, such as the `> ` of two paragraphs in one quote.
fn shared<'a>(a: &'a str, b: &str) -> &'a str {
    let n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    a.get(..n).unwrap_or("")
}

/// Closes a table row when `before`, the last text copied, is a cell, and puts the line that
/// marks a header under a header row of `cells` cells.
fn end_row(out: &mut String, before: Option<&Known>, cells: usize) {
    if let Some(known) = before
        && let Kind::Cell { head, .. } = known.kind
    {
        out.push_str(" |");
        if head {
            out.push('\n');
            out.push_str(&known.lead);
            out.push('|');
            out.push_str(&" --- |".repeat(cells));
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

/// The boxes behind a chip at `range` of a text: one for each line it is on, as wide as the part of
/// it on that line.
fn chip_quads(layout: &TextLayout, range: &Range<usize>) -> Vec<Bounds<Pixels>> {
    let (text, line) = (layout.text(), layout.line_height());
    let (Some(mut from), Some(part)) = (
        layout.position_for_index(range.start),
        text.get(range.clone()),
    ) else {
        return Vec::new();
    };
    let left = layout.bounds().left();
    let mut quads = Vec::new();
    let mut to = from;
    for (at, letter) in part.char_indices() {
        let Some(after) = layout.position_for_index(range.start + at + letter.len_utf8()) else {
            break;
        };
        // The place where a line wraps counts as the end of the line before it, so a letter that
        // ends on a lower line is the first of that line.
        if after.y > to.y {
            if to.x > from.x {
                quads.push(Bounds::from_corners(from, point(to.x, to.y + line)));
            }
            from = point(left, after.y);
        }
        to = after;
    }
    if to.x > from.x {
        quads.push(Bounds::from_corners(from, point(to.x, to.y + line)));
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
            selection.place_rows(&list);
            // A drag ends when the window loses focus, as its mouse-up may never arrive.
            if !window.is_window_active() {
                selection.dragging = false;
            }
            if selection.dragging
                && let Some(at) = selection.seen
            {
                // The list scrolled under a still pointer, by the wheel or by the drag, marks on
                // to where the pointer now is.
                if selection.drag_to(at) {
                    window.request_animation_frame();
                }
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
