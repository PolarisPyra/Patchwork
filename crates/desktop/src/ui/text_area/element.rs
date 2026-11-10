//! The text field drawn: its lines shaped and wrapped, the selection and the caret.

// Rust guideline compliant 2026-10-09

use std::ops::Range;

use gpui::{
    App, Bounds, CursorStyle, ElementId, ElementInputHandler, Entity, GlobalElementId, Hsla,
    LayoutId, MouseButton, PaintQuad, Pixels, Point, Role, SharedString, Style, TextRun,
    UnderlineStyle, Window, div, fill, point, prelude::*, px, relative, size,
};
use super::state::{DrawnLine, TextArea};

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
    pub(super) lines: Vec<DrawnLine>,
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
