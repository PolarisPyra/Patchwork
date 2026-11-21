//! Selecting on the terminal's screen: by letter, word, line or column block, held on the lines
//! it was made on as output arrives and the view scrolls, and followed over the whole window with
//! the view scrolling while the pointer is past its edge.

// Rust guideline compliant 2026-10-09

use std::time::Duration;

use gpui::{MouseDownEvent, MouseUpEvent, Pixels, Point, Task, prelude::*};

use crate::panel::Panel;
use crate::panel::cells::{line_at, ordered, word_at};
use crate::ui::selection::{Owner, claim, to_primary};

use super::shell::{Metrics, Shell};

/// How often a drag held past the screen's edge scrolls it on.
const SCROLL_EVERY: Duration = Duration::from_millis(40);

/// The most lines one step of that scrolling moves.
const SCROLL_MOST: f32 = 8.;

/// A place on the screen, as (line, column), where line 0 is the oldest line kept above it.
pub(in crate::panel) type Place = (i64, u16);

/// A selection: where it started and where it is, in either order.
pub(in crate::panel) type Span = (Place, Place);

/// What a drag selects by: the press's count picks it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::panel) enum Grain {
    #[default]
    Letter,
    Word,
    Line,
}

/// A drag selecting on the screen, while it is under way.
#[derive(Default)]
pub(in crate::panel) struct Drag {
    grain: Grain,
    /// What the press that began it selected.
    anchor: Span,
    /// Where the pointer last was, and the timer that scrolls while it is past the edge.
    pointer: Option<Point<Pixels>>,
    scrolling: Option<Task<()>>,
}

/// How many lines the screen keeps above what it shows.
fn kept(screen: &mut vt100::Screen) -> usize {
    let offset = screen.scrollback();
    screen.set_scrollback(usize::MAX);
    let kept = screen.scrollback();
    screen.set_scrollback(offset);
    kept
}

/// The line drawn at the top of the view.
pub(super) fn top_line(screen: &mut vt100::Screen) -> i64 {
    let offset = screen.scrollback();
    (kept(screen) - offset) as i64
}

/// The text of `span`, a column block when `block` is set, read from wherever it is in the
/// lines kept.
pub(super) fn text_of(screen: &mut vt100::Screen, span: Span, block: bool) -> String {
    let (a, b) = ordered(span);
    let offset = screen.scrollback();
    let kept = kept(screen) as i64;
    let (rows, cols) = screen.size();
    let (left, right) = (a.1.min(b.1), a.1.max(b.1));
    let last = b.0.min(kept + i64::from(rows) - 1);
    let mut text = String::new();
    let mut line = a.0.max(0);
    // A view's worth of lines at a time, scrolled so the first is in view: at the top while it
    // is above the screen.
    while line <= last {
        let (back, first) = if line < kept {
            ((kept - line) as usize, 0)
        } else {
            (0, (line - kept) as u16)
        };
        screen.set_scrollback(back);
        let end = (i64::from(first) + last - line).min(i64::from(rows) - 1) as u16;
        let end_line = line + i64::from(end - first);
        if block {
            for (row, piece) in screen
                .rows(left, right - left)
                .enumerate()
                .skip(usize::from(first))
                .take(usize::from(end - first) + 1)
            {
                text.push_str(piece.trim_end());
                if line + (row - usize::from(first)) as i64 != b.0 {
                    text.push('\n');
                }
            }
        } else {
            let from = if line == a.0 { a.1 } else { 0 };
            let to = if end_line == b.0 { b.1 } else { cols };
            text.push_str(&screen.contents_between(first, from, end, to));
            if end_line != b.0 && !screen.row_wrapped(end) {
                text.push('\n');
            }
        }
        line = end_line + 1;
    }
    screen.set_scrollback(offset);
    text
}

/// The rows of the view `span` covers, as (row, first column, column past the last).
pub(super) fn spans(screen: &mut vt100::Screen, span: Span, block: bool) -> Vec<(u16, u16, u16)> {
    let (a, b) = ordered(span);
    if a == b {
        return Vec::new();
    }
    let top = top_line(screen);
    let (rows, cols) = screen.size();
    let (left, right) = (a.1.min(b.1), a.1.max(b.1));
    (a.0.max(top)..=b.0.min(top + i64::from(rows) - 1))
        .map(|line| {
            let row = (line - top) as u16;
            if block {
                (row, left, right)
            } else {
                let from = if line == a.0 { a.1 } else { 0 };
                let to = if line == b.0 { b.1 } else { cols };
                (row, from, to)
            }
        })
        .collect()
}

/// `span` of the view's rows, as lines.
fn placed(((r0, c0), (r1, c1)): ((u16, u16), (u16, u16)), top: i64) -> Span {
    ((top + i64::from(r0), c0), (top + i64::from(r1), c1))
}

/// The selected text, unless nothing is selected.
pub(super) fn selected_text(shell: &mut Shell) -> Option<String> {
    let span = shell.selection.filter(|(a, b)| a != b)?;
    let text = text_of(shell.parser.screen_mut(), span, shell.block);
    (!text.is_empty()).then_some(text)
}

impl Shell {
    /// Feeds `data` to the screen, keeping the selection on the text it was made on. Lines
    /// that fall off the oldest end move it up; when its text is gone or written over, it goes.
    pub(super) fn process(&mut self, data: &[u8]) {
        let Some(span) = self.selection else {
            return self.parser.process(data);
        };
        let before = text_of(self.parser.screen_mut(), span, self.block);
        self.parser.process(data);
        let rows = i64::from(self.parser.screen().size().0);
        let screen = self.parser.screen_mut();
        self.selection = (0..=rows)
            .map(|up| ((span.0.0 - up, span.0.1), (span.1.0 - up, span.1.1)))
            .take_while(|moved| moved.0.0.min(moved.1.0) >= 0)
            .find(|&moved| text_of(screen, moved, self.block) == before);
    }
}

impl Panel {
    /// A left press on the screen that the program there does not hear.
    pub(super) fn press(&mut self, e: &MouseDownEvent, metrics: &Metrics, cx: &mut Context<Self>) {
        let cell = self.cell_at(e.position, metrics);
        let under = self.cell_under(e.position, metrics);
        let grain = match e.click_count {
            0 | 1 => Grain::Letter,
            2 => Grain::Word,
            _ => Grain::Line,
        };
        let Some(shell) = self.active_shell_mut() else {
            return;
        };
        let top = top_line(shell.parser.screen_mut());
        let screen = shell.parser.screen();
        // Shift+click carries the selection on to here.
        let anchor = match (shell.selection, e.modifiers.shift && grain == Grain::Letter) {
            (Some((from, _)), true) => (from, placed((cell, cell), top).0),
            _ => match grain {
                // A double click takes the word, a triple click the line, as in xterm.
                Grain::Letter => placed((cell, cell), top),
                Grain::Word => placed(word_at(screen, under), top),
                Grain::Line => placed(line_at(screen, under.0), top),
            },
        };
        shell.selection = Some(anchor);
        // Alt+drag selects a block of columns; Alt+click alone still moves the cursor there.
        shell.block = e.modifiers.alt && grain == Grain::Letter;
        self.drag = Drag {
            grain,
            anchor,
            pointer: None,
            scrolling: None,
        };
        self.selecting = true;
        if anchor.0 != anchor.1 {
            self.settle(cx);
        }
        cx.notify();
    }

    /// The pointer moved anywhere in the window during a drag that began on the screen.
    pub(super) fn drag_to(
        &mut self,
        position: Point<Pixels>,
        metrics: &Metrics,
        cx: &mut Context<Self>,
    ) {
        self.drag.pointer = Some(position);
        self.extend(position, metrics, cx);
        self.keep_scrolling(metrics, cx);
    }

    /// The drag ended, wherever the pointer is.
    pub(super) fn release(&mut self, e: &MouseUpEvent, metrics: &Metrics, cx: &mut Context<Self>) {
        self.drag.scrolling = None;
        self.drag.pointer = None;
        if !std::mem::take(&mut self.selecting) {
            return;
        }
        let alt_click = self.settings.alt_click_moves_cursor;
        let Some(shell) = self.active_shell_mut() else {
            return;
        };
        if shell.selection.is_some_and(|(a, b)| a == b) {
            shell.selection = None;
            if std::mem::take(&mut shell.block) && alt_click {
                self.alt_click(e.position, metrics);
            }
        } else {
            self.settle(cx);
            if self.settings.copy_on_selection {
                self.copy_selection(cx);
            }
        }
        cx.notify();
    }

    /// Selects every line kept above the screen and on it, down to the last one written.
    pub(super) fn select_all(&mut self, cx: &mut Context<Self>) {
        let Some(shell) = self.active_shell_mut() else {
            return;
        };
        let screen = shell.parser.screen_mut();
        let (rows, cols) = screen.size();
        let offset = screen.scrollback();
        screen.set_scrollback(0);
        let written = screen
            .rows(0, cols)
            .enumerate()
            .filter(|(_, row)| !row.trim().is_empty())
            .map(|(row, _)| row)
            .last()
            .unwrap_or(usize::from(rows) - 1);
        screen.set_scrollback(offset);
        let last = kept(screen) as i64 + written as i64;
        shell.selection = Some(((0, 0), (last, cols)));
        shell.block = false;
        self.settle(cx);
        cx.notify();
    }

    /// Makes the terminal's selection the latest, and the primary selection.
    fn settle(&mut self, cx: &mut Context<Self>) {
        claim(Owner::Terminal, cx);
        if let Some(text) = self.active_shell_mut().and_then(selected_text) {
            to_primary(&text, cx);
        }
    }

    /// Moves the selection's end to the cell under `position`, by the drag's grain.
    fn extend(&mut self, position: Point<Pixels>, metrics: &Metrics, cx: &mut Context<Self>) {
        let cell = self.cell_at(position, metrics);
        let under = self.cell_under(position, metrics);
        let (grain, anchor) = (self.drag.grain, self.drag.anchor);
        let Some(shell) = self.active_shell_mut() else {
            return;
        };
        let top = top_line(shell.parser.screen_mut());
        let screen = shell.parser.screen();
        let reach = match grain {
            Grain::Letter => placed((cell, cell), top),
            Grain::Word => placed(word_at(screen, under), top),
            Grain::Line => placed(line_at(screen, under.0), top),
        };
        let span = match (grain, shell.selection) {
            (Grain::Letter, Some((from, _))) => (from, reach.0),
            _ if ordered(reach).0 < ordered(anchor).0 => (ordered(anchor).1, ordered(reach).0),
            _ => (ordered(anchor).0, ordered(reach).1),
        };
        if shell.selection == Some(span) {
            return;
        }
        shell.selection = Some(span);
        if span.0 != span.1 {
            claim(Owner::Terminal, cx);
        }
        cx.notify();
    }

    /// How far the pointer is above (< 0) or below (> 0) the screen, in lines.
    fn past_edge(&self, metrics: &Metrics) -> f32 {
        let Some(at) = self.drag.pointer else {
            return 0.;
        };
        let rows = f32::from(self.grid.map_or(1, |(_, rows)| rows));
        let y = f32::from(at.y - self.origin.y) / metrics.line_h;
        if y < 0. { y } else { (y - rows).max(0.) }
    }

    /// While the pointer is held above or below the screen, the view scrolls through the lines
    /// kept, faster the further away it is, and the selection follows.
    fn keep_scrolling(&mut self, metrics: &Metrics, cx: &mut Context<Self>) {
        if self.past_edge(metrics) == 0. {
            self.drag.scrolling = None;
            return;
        }
        if self.drag.scrolling.is_some() {
            return;
        }
        let metrics = metrics.clone();
        self.drag.scrolling = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SCROLL_EVERY).await;
                let more = this
                    .update(cx, |this, cx| {
                        let past = this.past_edge(&metrics);
                        let Some(at) = this.drag.pointer.filter(|_| this.selecting && past != 0.)
                        else {
                            return false;
                        };
                        if let Some(shell) = this.active_shell_mut() {
                            let screen = shell.parser.screen_mut();
                            let lines = past.abs().ceil().min(SCROLL_MOST) as usize;
                            let back = if past < 0. {
                                screen.scrollback() + lines
                            } else {
                                screen.scrollback().saturating_sub(lines)
                            };
                            screen.set_scrollback(back);
                        }
                        this.extend(at, &metrics, cx);
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !more {
                    break;
                }
            }
        }));
    }
}
