//! Selecting text that is only read: a drag over the chat's text marks it, across paragraphs,
//! headings, table cells and code, and Ctrl+C copies what is marked.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, CursorStyle, DispatchPhase, Element, ElementId, GlobalElementId,
    Hitbox, HitboxBehavior, InspectorElementId, IntoElement, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, StyledText,
    TextLayout, Window, actions, fill, point,
};

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
}

/// What is marked, and the texts a drag can reach.
#[derive(Default)]
struct Selection {
    /// The texts drawn since the chat was last laid out, which is every one on screen.
    drawn: Vec<Drawn>,
    /// The marked part of each text, by the text's id.
    marked: HashMap<u64, Range<usize>>,
    /// The marked text, top to bottom, as Ctrl+C copies it.
    text: String,
    /// The text the drag began in, and where in it.
    anchor: Option<(u64, usize)>,
    dragging: bool,
    /// Where the pointer was when the drag was last worked out: every text hears each move.
    seen: Option<Point<Pixels>>,
}

impl gpui::Global for Selection {}

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
}

/// Wraps `text` so a drag can mark it.
pub fn selectable(id: u64, text: StyledText) -> Selectable {
    Selectable { id, text }
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
        let marked = cx
            .default_global::<Selection>()
            .marked
            .get(&self.id)
            .cloned();
        if let Some(range) = marked {
            let color = theme(cx).accent.opacity(0.3);
            for quad in quads(&layout, &range) {
                window.paint_quad(fill(quad, color));
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
        });

        let (id, hitbox) = (self.id, prepaint.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if event.button != MouseButton::Left {
                return;
            }
            let selection = cx.default_global::<Selection>();
            match phase {
                // A press anywhere drops what was marked; the text under it then starts anew.
                DispatchPhase::Capture => {
                    selection.anchor = None;
                    selection.dragging = false;
                    if !selection.marked.is_empty() {
                        selection.marked.clear();
                        selection.text.clear();
                        window.refresh();
                    }
                }
                DispatchPhase::Bubble if hitbox.is_hovered(window) => {
                    let at = match layout.index_for_position(event.position) {
                        Ok(at) | Err(at) => at,
                    };
                    selection.anchor = Some((id, at));
                    selection.dragging = true;
                    selection.seen = None;
                }
                DispatchPhase::Bubble => {}
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

impl Selection {
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
        let mut pieces = Vec::new();
        for (i, drawn) in self.drawn.iter().enumerate().take(last + 1).skip(first) {
            let text = drawn.layout.text();
            let start = if i == first { first_at } else { 0 };
            let end = if i == last { last_at } else { text.len() };
            let (start, end) = (start.min(text.len()), end.min(text.len()));
            if start < end && text.is_char_boundary(start) && text.is_char_boundary(end) {
                pieces.push(text[start..end].to_string());
                self.marked.insert(drawn.id, start..end);
            }
        }
        self.text = pieces.join("\n");
    }
}

/// The boxes behind `range` of a text: one for a single line, or the end of the first line, the
/// lines between, and the start of the last.
fn quads(layout: &TextLayout, range: &Range<usize>) -> Vec<Bounds<Pixels>> {
    let (Some(start), Some(end)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return Vec::new();
    };
    let (bounds, line) = (layout.bounds(), layout.line_height());
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

/// An id for the `nth` text drawn under `key`.
pub fn text_id(key: &SharedString, nth: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (key, nth).hash(&mut hasher);
    hasher.finish()
}
