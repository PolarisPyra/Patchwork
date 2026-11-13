//! The text field drawn: its lines shaped and wrapped, the selection and the caret. A single-line
//! field never wraps; it scrolls sideways to keep the caret in view.

// Rust guideline compliant 2026-10-09

use std::ops::Range;

use gpui::{
    App, Bounds, CursorStyle, FontStyle, FontWeight, StrikethroughStyle, ElementId,
    ElementInputHandler, Entity, GlobalElementId, Hsla, LayoutId, MouseButton, PaintQuad, Pixels,
    Point, Role, SharedString, Style, TextRun, UnderlineStyle, Window, div, fill, point,
    prelude::*, px, relative, size,
};
use super::markdown;
use super::state::{DrawnLine, TextArea};
use crate::ui::theme::FONT_MONO;

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
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::top))
            .on_action(cx.listener(Self::bottom))
            .on_action(cx.listener(Self::select_top))
            .on_action(cx.listener(Self::select_bottom))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::select_page_up))
            .on_action(cx.listener(Self::select_page_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::open_menu))
            .w_full()
            .child(TextElement { area: cx.entity() })
            .children(
                self.menu
                    .as_ref()
                    .map(|(menu, at)| crate::ui::menu::Menu::view(menu, *at)),
            )
    }
}

/// Lays the text out at the field's width and draws it with its selection and caret.
struct TextElement {
    area: Entity<TextArea>,
}

struct Prepainted {
    pub(super) lines: Vec<DrawnLine>,
    quads: Vec<PaintQuad>,
    caret: Option<PaintQuad>,
    revealed: (usize, usize),
    scroll_x: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// How a run of text is drawn: its colours and the Markdown style of each byte, if the field
/// styles Markdown.
#[derive(Clone, Copy)]
struct Look<'a> {
    color: Hsla,
    mark_color: Hsla,
    styles: Option<&'a [u8]>,
}

/// Shapes `text` one `\n`-line at a time, wrapped to `width` when there is one.
fn shape(
    text: &str,
    look: Look<'_>,
    width: Option<Pixels>,
    line_height: Pixels,
    marked: Option<&Range<usize>>,
    window: &mut Window,
) -> Vec<DrawnLine> {
    let style = window.text_style();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let base = style.font();
    let underline = UnderlineStyle {
        color: Some(look.color),
        thickness: px(1.),
        wavy: false,
    };
    // The look of the byte at `i`: its Markdown style, and whether the input method marks it.
    let key = |i: usize| {
        let flags = look.styles.and_then(|s| s.get(i).copied()).unwrap_or(0);
        (flags, marked.is_some_and(|m| m.contains(&i)))
    };
    let run = |len: usize, (flags, composing): (u8, bool)| {
        let mut font = base.clone();
        if flags & markdown::BOLD != 0 {
            font.weight = FontWeight::BOLD;
        }
        if flags & markdown::ITALIC != 0 {
            font.style = FontStyle::Italic;
        }
        if flags & markdown::CODE != 0 {
            font.family = FONT_MONO.into();
        }
        TextRun {
            len,
            font,
            color: if flags & markdown::MARK != 0 {
                look.mark_color
            } else {
                look.color
            },
            background_color: None,
            underline: composing.then_some(underline),
            strikethrough: (flags & markdown::STRIKE != 0).then_some(StrikethroughStyle {
                color: Some(look.color),
                thickness: px(1.),
            }),
        }
    };
    let mut lines = Vec::new();
    let mut start = 0;
    let mut top = px(0.);
    for line in text.split('\n') {
        let end = start + line.len();
        let mut runs = Vec::new();
        let mut from = start;
        for (i, _) in line.char_indices().map(|(i, c)| (start + i, c)).skip(1) {
            if key(i) != key(from) {
                runs.push(run(i - from, key(from)));
                from = i;
            }
        }
        runs.push(run(end - from, key(from)));
        let shaped = window
            .text_system()
            .shape_text(
                SharedString::from(line.to_string()),
                font_size,
                &runs,
                width,
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
        let styles = area.styles();
        let wraps = !area.single_line;
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let layout = window.request_measured_layout(style, move |known, available, window, _| {
            let width = known.width.unwrap_or(match available.width {
                gpui::AvailableSpace::Definite(w) => w,
                _ => px(10_000.),
            });
            let look = Look {
                color: gpui::white(),
                mark_color: gpui::white(),
                styles: styles.as_deref(),
            };
            let lines = shape(
                &text,
                look,
                wraps.then_some(width),
                line_height,
                None,
                window,
            );
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
            Look {
                color,
                mark_color: area.placeholder_color,
                styles: area.styles().as_deref(),
            },
            (!area.single_line).then_some(bounds.size.width),
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
        // A single line wider than the field scrolls just far enough to show the caret.
        let scroll_x = if area.single_line {
            let end = lines
                .first()
                .and_then(|l| l.layout.position_for_index(text.len(), line_height))
                .map_or(px(0.), |p| p.x);
            let room = (bounds.size.width - px(2.)).max(px(0.));
            let caret = at(cursor).map_or(px(0.), |p| p.x);
            let mut x = area.scroll_x;
            if caret - x > room {
                x = caret - room;
            }
            if caret < x {
                x = caret;
            }
            x.min((end - room).max(px(0.))).max(px(0.))
        } else {
            px(0.)
        };
        let origin = bounds.origin - point(scroll_x, px(0.));
        let mut quads = Vec::new();
        if !selected.is_empty()
            && let (Some(a), Some(b)) = (at(selected.start), at(selected.end))
        {
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
        // When the caret or the text changed, the box the field scrolls in brings the caret's line
        // into view, as a new line from Shift+Enter at the bottom would otherwise be hidden.
        let revealed = (area.cursor(), area.content.len());
        let view = area.scroll.bounds();
        if revealed != area.revealed
            && view.size.height > px(0.)
            && let Some(p) = at(area.shown_offset(area.cursor()))
        {
            let (top, bottom) = (bounds.top() + p.y, bounds.top() + p.y + line_height);
            let mut offset = area.scroll.offset();
            if top < view.top() {
                offset.y += view.top() - top;
            } else if bottom > view.bottom() {
                offset.y -= bottom - view.bottom();
            }
            if offset != area.scroll.offset() {
                area.scroll.set_offset(offset);
                window.request_animation_frame();
            }
        }
        let caret = selected.is_empty().then(|| at(cursor)).flatten().map(|p| {
            fill(
                Bounds::new(origin + p, size(px(1.), line_height)),
                caret_color,
            )
        });
        Prepainted {
            lines,
            quads,
            caret,
            revealed,
            scroll_x,
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
        // Where the text starts, left of the field by however far it is scrolled. Positions from
        // the mouse and the input method are measured from here.
        let scroll_x = prepaint.scroll_x;
        let text_bounds = Bounds::new(bounds.origin - point(scroll_x, px(0.)), bounds.size);
        window.handle_input(
            &focus,
            ElementInputHandler::new(text_bounds, self.area.clone()),
            cx,
        );
        let line_height = window.line_height();
        let mask = gpui::ContentMask { bounds };
        window.with_content_mask(Some(mask), |window| {
            for quad in prepaint.quads.drain(..) {
                window.paint_quad(quad);
            }
            for line in &prepaint.lines {
                if let Err(e) = line.layout.paint(
                    text_bounds.origin + point(px(0.), line.top),
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
        });
        let lines = std::mem::take(&mut prepaint.lines);
        let revealed = prepaint.revealed;
        self.area.update(cx, |area, _| {
            area.revealed = revealed;
            area.lines = lines;
            area.bounds = Some(text_bounds);
            area.scroll_x = scroll_x;
            area.line_height = line_height;
        });
    }
}
