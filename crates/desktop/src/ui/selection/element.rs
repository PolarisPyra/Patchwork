//! A text a drag can mark, as an element: it draws the marks, chips and links, and takes the clicks.

// Rust guideline compliant 2026-10-09

use std::ops::Range;
use std::rc::Rc;

use gpui::{
    App, Bounds, ClipboardItem, CursorStyle, DispatchPhase, Element, ElementId, GlobalElementId,
    Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, StyledText,
    TextLayout, Window, fill, point, px,
};

use super::owner::{Owner, claim, to_primary};
use super::state::{CHIP_INSET_Y, CHIP_PAD_X, Drawn, Kind, Known, Mark, Place, Selection, Unit};
use super::text::word_at;
use crate::ui::menu::{Dismissed, Menu, MenuItem};
use crate::ui::theme::{radius_sm, theme};

/// A text that a drag can mark. `id` tells it from every other text on screen, and stays the
/// same from one frame to the next.
pub struct Selectable {
    pub(super) id: u64,
    pub(super) text: StyledText,
    pub(super) prefix: Option<SharedString>,
    /// How far left of the text its prefix is drawn: a press there starts a drag too.
    pub(super) gutter: Pixels,
    /// How far right of the text a mark that runs on past its end reaches, and a press counts.
    pub(super) reach: Pixels,
    pub(super) kind: Kind,
    /// What each copied line of the text starts with, for the quotes and list items it is in.
    pub(super) lead: SharedString,
    /// The Markdown that was around each styled range, put back when the range is copied.
    pub(super) marks: Vec<Mark>,
    /// Ranges drawn as rounded chips behind the text, each with its colour.
    pub(super) chips: Vec<(Range<usize>, Hsla)>,
    /// Ranges a click opens, each with its address.
    pub(super) links: Vec<(Range<usize>, SharedString)>,
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
        // With everything marked, each text is marked whole as it is drawn.
        if selection.all {
            selection.marked.insert(self.id, 0..layout.len());
        }
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
            hitbox: prepaint.clone(),
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
            let extend = event.modifiers.shift && selection.anchor.is_some();
            match event.button {
                // A press anywhere drops what was marked, unless it is on the open menu, which
                // keeps it to copy, or Shift holds it to reach further; the text under it then
                // starts anew.
                MouseButton::Left if phase == DispatchPhase::Capture => {
                    if selection.menu.is_some() || extend {
                        return;
                    }
                    selection.anchor = None;
                    selection.dragging = false;
                    selection.unit = Unit::Letter;
                    // A new selection starts from the texts on screen.
                    let Selection { drawn, known, .. } = &mut *selection;
                    known.retain(|id, _| drawn.iter().any(|drawn| drawn.id == *id));
                    if !selection.marked.is_empty() || selection.all {
                        selection.unmark();
                        window.refresh();
                    }
                }
                MouseButton::Left if hovered => {
                    // The press is the chat's: what is under it is not the window's to focus.
                    window.prevent_default();
                    let (at, text) = (at(), layout.text());
                    selection.dragging = true;
                    selection.seen = None;
                    if extend {
                        // Shift+click reaches from where the selection began to here.
                        selection.survey(window);
                        selection.drag_to(event.position);
                    } else {
                        match event.click_count {
                            0 | 1 => {
                                selection.anchor = Some((id, at..at));
                                selection.unit = Unit::Letter;
                            }
                            2 => {
                                let word = word_at(&text, at);
                                selection.anchor = Some((id, word.clone()));
                                selection.unit = Unit::Word;
                                selection.mark(id, word, &text);
                            }
                            _ => {
                                selection.anchor = Some((id, 0..text.len()));
                                selection.unit = Unit::Whole;
                                selection.mark(id, 0..text.len(), &text);
                            }
                        }
                    }
                    if !selection.marked.is_empty() {
                        claim(Owner::Chat, cx);
                    }
                    window.refresh();
                }
                // A right click keeps what is marked under it, or else marks the word there.
                MouseButton::Right if hovered => {
                    let (at, text) = (at(), layout.text());
                    let on_marked = selection.all
                        || selection
                            .marked
                            .get(&id)
                            .is_some_and(|range| range.start <= at && at <= range.end);
                    if !on_marked {
                        selection.anchor = None;
                        selection.dragging = false;
                        selection.mark(id, word_at(&text, at), &text);
                        if !selection.marked.is_empty() {
                            claim(Owner::Chat, cx);
                        }
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
            selection.survey(window);
            if selection.drag_to(event.position) {
                if !selection.marked.is_empty() {
                    claim(Owner::Chat, cx);
                }
                window.refresh();
            }
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if event.button != MouseButton::Left {
                return;
            }
            let selection = cx.default_global::<Selection>();
            // What a drag settles on goes to the primary selection, once, as the button lets go.
            if std::mem::take(&mut selection.dragging) && phase == DispatchPhase::Capture {
                let plain = selection.plain.clone();
                to_primary(&plain, cx);
                return;
            }
            let selection = cx.default_global::<Selection>();
            // A click that pressed and let go on the same link, marking nothing, opens it.
            if phase == DispatchPhase::Bubble
                && selection.marked.is_empty()
                && let Some((anchor, from)) = &selection.anchor
                && *anchor == id
                && up_hitbox.is_hovered(window)
                && let Some(link) = link_at(&up_layout, &up_links, event.position)
                && up_links[link].0.contains(&from.start)
            {
                cx.open_url(&up_links[link].1);
            }
        });
    }
}

/// Which of `links` is under the pointer, when it is over the text.
pub(super) fn link_under(
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
pub(super) fn link_at(
    layout: &TextLayout,
    links: &[(Range<usize>, SharedString)],
    position: Point<Pixels>,
) -> Option<usize> {
    let at = layout.index_for_position(position).ok()?;
    links.iter().position(|(range, _)| range.contains(&at))
}

/// Opens the menu for a text at `at`, with the link under the pointer, if any.
pub(super) fn open_menu(
    link: Option<SharedString>,
    at: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let selection = cx.default_global::<Selection>();
    let (marked, plain) = (selection.text.clone(), selection.plain.clone());
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
                cx.write_to_clipboard(ClipboardItem::new_string(plain.clone()));
            })
            .icon("copy"),
        );
        items.push(
            MenuItem::new("Copy as Markdown", move |_, cx| {
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

/// The boxes behind `range` of a text: one for a single line, or the end of the first line, the
/// lines between, and the start of the last.
pub(super) fn quads(
    layout: &TextLayout,
    range: &Range<usize>,
    to_edge: bool,
) -> Vec<Bounds<Pixels>> {
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
pub(super) fn chip_quads(layout: &TextLayout, range: &Range<usize>) -> Vec<Bounds<Pixels>> {
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
