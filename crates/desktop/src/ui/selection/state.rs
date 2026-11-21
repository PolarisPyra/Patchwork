//! What is marked: the texts on screen, the drag over them, and the overlay that fills between them.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use gpui::{
    AnyElement, App, Bounds, ClipboardItem, ContentMask, DispatchPhase, Entity, Hitbox,
    HitboxBehavior, IntoElement, KeyBinding, ListState, MouseButton, MouseDownEvent, Pixels, Point,
    SharedString, Styled, TextLayout, Window, actions, canvas, fill, point, px,
};

use super::owner::{Owner, claim, to_primary};
use super::text::{markdown, plain, word_at};
use crate::ui::menu::Menu;
use crate::ui::theme::theme;

/// How far a chip reaches past its first and last letter, and how far in from its line's top and
/// bottom it stops, so chips on lines above each other keep apart.
pub(super) const CHIP_PAD_X: f32 = 3.;
pub(super) const CHIP_INSET_Y: f32 = 1.;

actions!(selection, [CopySelection, SelectAllText, ClearSelection]);

/// The keys that copy, mark all and drop the marked text, where no text field has the keys.
pub fn bindings() -> Vec<KeyBinding> {
    let context = Some("Workspace");
    vec![
        KeyBinding::new("ctrl-c", CopySelection, context),
        KeyBinding::new("ctrl-a", SelectAllText, context),
        KeyBinding::new("escape", ClearSelection, context),
    ]
}

/// One text drawn this frame, where a pointer can reach it.
pub(super) struct Drawn {
    pub(super) id: u64,
    pub(super) layout: TextLayout,
    /// How far left of the text its prefix is drawn.
    pub(super) gutter: Pixels,
    /// How far right of the text a mark that runs on past its end reaches.
    pub(super) reach: Pixels,
    /// The part of the text not clipped by the list it scrolls in.
    pub(super) visible: Bounds<Pixels>,
    /// Where a press reaches it, unless something is drawn over it.
    pub(super) hitbox: Hitbox,
}

/// Where a text sits in reading order: the list row it is in, then how far down and across that
/// row. A text outside a list uses row 0 and its place on screen.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct Place {
    pub(super) row: usize,
    pub(super) y: f32,
    pub(super) x: f32,
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

/// What a drag marks by: the letter, the word, or the whole text, as one, two or three clicks
/// began it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Unit {
    #[default]
    Letter,
    Word,
    Whole,
}

/// A text seen since the selection began, kept after it scrolls away so a mark can run past it.
pub(super) struct Known {
    pub(super) place: Place,
    pub(super) layout: TextLayout,
    pub(super) prefix: Option<SharedString>,
    pub(super) kind: Kind,
    pub(super) lead: SharedString,
    pub(super) marks: Vec<Mark>,
}

/// What is marked, and the texts a drag can reach.
#[derive(Default)]
pub(super) struct Selection {
    /// The texts drawn since the chat was last laid out, which is every one on screen.
    pub(super) drawn: Vec<Drawn>,
    /// Every text seen since the selection began, by id, on screen or not.
    pub(super) known: HashMap<u64, Known>,
    /// The marked part of each text, by the text's id.
    pub(super) marked: HashMap<u64, Range<usize>>,
    /// The last text marked, top to bottom: the only one whose highlight stops where its mark does.
    pub(super) last: Option<u64>,
    /// The marked text, top to bottom, as Markdown, for the menu's Copy as Markdown.
    pub(super) text: String,
    /// The marked text as it reads, with no Markdown, as Ctrl+C copies it.
    pub(super) plain: String,
    /// Everything is marked, by Ctrl+A: each text drawn is marked whole.
    pub(super) all: bool,
    /// The text the drag began in, and the part of it the press took: a place, a word or the
    /// whole text.
    pub(super) anchor: Option<(u64, Range<usize>)>,
    /// What the drag marks by.
    pub(super) unit: Unit,
    pub(super) dragging: bool,
    /// Where the pointer was when the drag was last worked out: every text hears each move.
    pub(super) seen: Option<Point<Pixels>>,
    /// The texts on screen that something drawn over them, such as a dialog, keeps the pointer
    /// from. A drag that starts above them passes them by.
    pub(super) covered: HashSet<u64>,
    /// The right-click menu, while open, and where it opened.
    pub(super) menu: Option<(Entity<Menu>, Point<Pixels>)>,
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

/// Copies the marked text as it reads, if any is.
pub fn copy(cx: &mut App) {
    let text = cx.default_global::<Selection>().plain.clone();
    if !text.is_empty() {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }
}

/// Marks every text, as Ctrl+A does, copying as `markdown` and `plain`: the whole chat, which
/// holds more than is drawn.
pub fn select_all(markdown: String, plain: String, window: &mut Window, cx: &mut App) {
    to_primary(&plain, cx);
    let selection = cx.default_global::<Selection>();
    selection.anchor = None;
    selection.dragging = false;
    selection.marked.clear();
    selection.last = None;
    selection.all = true;
    selection.text = markdown;
    selection.plain = plain;
    claim(Owner::Chat, cx);
    window.refresh();
}

/// Drops what is marked, as Escape does. Returns whether anything was.
pub fn clear(window: &mut Window, cx: &mut App) -> bool {
    let selection = cx.default_global::<Selection>();
    if selection.marked.is_empty() && !selection.all {
        return false;
    }
    selection.unmark();
    window.refresh();
    true
}

impl Selection {
    /// Drops every mark.
    pub(super) fn unmark(&mut self) {
        self.marked.clear();
        self.text.clear();
        self.plain.clear();
        self.all = false;
    }

    /// Marks `range` of the one text `id`, whose words are `text`, and nothing else.
    pub(super) fn mark(&mut self, id: u64, range: Range<usize>, text: &str) {
        self.unmark();
        if range.start < range.end
            && range.end <= text.len()
            && text.is_char_boundary(range.start)
            && text.is_char_boundary(range.end)
        {
            self.plain = text[range.clone()].to_string();
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
    pub(super) fn gaps(&self) -> Vec<Bounds<Pixels>> {
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
    pub(super) fn place_rows(&mut self, list: &ListState) {
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

    /// Finds which texts on screen something drawn over them covers, by asking whether a press at
    /// any corner or the middle of each would reach it. A text the list has scrolled out of view,
    /// which no press reaches, isn't covered: a drag marks on through it.
    pub(super) fn survey(&mut self, window: &Window) {
        self.covered = self
            .drawn
            .iter()
            .filter(|drawn| {
                let v = drawn.visible;
                if v.is_empty() {
                    return false;
                }
                let (left, right) = (v.left(), (v.right() - px(1.)).max(v.left()));
                let (top, bottom) = (v.top(), (v.bottom() - px(1.)).max(v.top()));
                ![
                    v.center(),
                    point(left, top),
                    point(right, top),
                    point(left, bottom),
                    point(right, bottom),
                ]
                .into_iter()
                .any(|at| drawn.hitbox.is_hovered_at(at, window))
            })
            .map(|drawn| drawn.id)
            .collect();
    }

    /// The text at `position` and the place in it, among the texts in view: the one under the
    /// pointer, or else the last one that starts above it, or else the first.
    pub(super) fn hit(&self, position: Point<Pixels>) -> Option<(u64, usize)> {
        let mut shown: Vec<&Drawn> = self
            .drawn
            .iter()
            .filter(|drawn| !drawn.visible.is_empty() && !self.covered.contains(&drawn.id))
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
    pub(super) fn drag_to(&mut self, position: Point<Pixels>) -> bool {
        let Some((anchor, held)) = self.anchor.clone() else {
            return false;
        };
        let Some((to, to_at)) = self.hit(position) else {
            return false;
        };
        let (Some(from_place), Some(to_known)) = (
            self.known.get(&anchor).map(|known| known.place),
            self.known.get(&to),
        ) else {
            return false;
        };
        let to_place = to_known.place;
        // A drag begun by a double or triple click takes whole words or whole texts.
        let reach = match self.unit {
            Unit::Letter => to_at..to_at,
            Unit::Word => word_at(&to_known.layout.text(), to_at),
            Unit::Whole => 0..to_known.layout.len(),
        };
        let ((first, first_place, first_at), (last, last_place, last_at)) =
            if (from_place, held.start) <= (to_place, reach.start) {
                ((anchor, from_place, held.start), (to, to_place, reach.end))
            } else {
                ((to, to_place, reach.start), (anchor, from_place, held.end))
            };
        let mut between: Vec<(Place, u64)> = self
            .known
            .iter()
            .filter(|(id, known)| {
                first_place <= known.place
                    && known.place <= last_place
                    && !self.covered.contains(id)
            })
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
        self.plain = plain(&pieces);
        self.all = false;
        self.marked != before
    }
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
                // A list under a dialog stays still while a drag runs over the dialog.
                let under = !selection.drawn.is_empty()
                    && selection
                        .drawn
                        .iter()
                        .all(|drawn| selection.covered.contains(&drawn.id));
                let past = if under {
                    Pixels::ZERO
                } else if at.y < bounds.top() {
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

/// Drawn behind a scrolling list of texts, before them: a press beside or between its texts,
/// which nothing in the list took, starts a drag from the nearest text, as one on it would.
pub fn margin() -> impl IntoElement {
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        |_, hitbox, window, _| {
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || window.default_prevented()
                    || !hitbox.is_hovered(window)
                {
                    return;
                }
                let selection = cx.default_global::<Selection>();
                if selection.menu.is_some() {
                    return;
                }
                selection.survey(window);
                if event.modifiers.shift && selection.anchor.is_some() {
                    selection.drag_to(event.position);
                } else {
                    let Some((id, at)) = selection.hit(event.position) else {
                        return;
                    };
                    selection.anchor = Some((id, at..at));
                    selection.unit = Unit::Letter;
                }
                selection.dragging = true;
                selection.seen = None;
                if !selection.marked.is_empty() {
                    claim(Owner::Chat, cx);
                }
                window.refresh();
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
