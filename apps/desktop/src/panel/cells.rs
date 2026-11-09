//! How a shell's screen is drawn: its rows of styled cells, the selection over them and the
//! cursor, in the colours xterm gives them.

use std::ops::Range;

use gpui::{FontStyle, HighlightStyle, Hsla, StyledText, UnderlineStyle, div, prelude::*, px, rgb};
use patchwork_server::settings::{CursorStyle, TerminalSettings};

use super::input::ensure_contrast;
use super::terminal::Metrics;
use crate::ui::theme::Theme;

/// The ANSI colors for the dark and light themes.
const ANSI_DARK: [u32; 16] = [
    0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666,
    0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xe5e5e5,
];
const ANSI_LIGHT: [u32; 16] = [
    0x000000, 0xcd3131, 0x107c10, 0x949800, 0x0451a5, 0xbc05bc, 0x0598bc, 0x555555, 0x666666,
    0xf14c4c, 0x14ce14, 0xb5ba00, 0x3b8eea, 0xd670d6, 0x29b8db, 0xa5a5a5,
];

/// What a double click stops a word at, xterm's `wordSeparator`.
const WORD_SEPARATORS: &str = " ()[]{}',\"`";

/// The selection, first cell first.
pub(super) fn ordered<T: Ord>((a, b): (T, T)) -> (T, T) {
    if a <= b { (a, b) } else { (b, a) }
}

/// The word, or run of separators, around `cell`, as a selection.
pub(super) fn word_at(screen: &vt100::Screen, (row, col): (u16, u16)) -> ((u16, u16), (u16, u16)) {
    let cols = screen.size().1;
    let separator = |c: u16| {
        screen.cell(row, c).is_none_or(|cell| {
            let s = cell.contents();
            s.is_empty() || s.chars().all(|ch| WORD_SEPARATORS.contains(ch))
        })
    };
    let kind = separator(col);
    let mut from = col;
    while from > 0 && separator(from - 1) == kind {
        from -= 1;
    }
    let mut to = col + 1;
    while to < cols && separator(to) == kind {
        to += 1;
    }
    ((row, from), (row, to))
}

/// The whole of `row`, as a selection.
pub(super) fn line_at(screen: &vt100::Screen, row: u16) -> ((u16, u16), (u16, u16)) {
    ((row, 0), (row, screen.size().1))
}

/// A cell's attributes, which runs of equal ones share.
#[derive(Clone, Copy, PartialEq)]
struct Attrs {
    fg: vt100::Color,
    bg: vt100::Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

impl Attrs {
    fn of(cell: &vt100::Cell) -> Self {
        Self {
            fg: cell.fgcolor(),
            bg: cell.bgcolor(),
            bold: cell.bold(),
            dim: cell.dim(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
        }
    }

    fn plain(&self) -> bool {
        *self
            == Self {
                fg: vt100::Color::Default,
                bg: vt100::Color::Default,
                bold: false,
                dim: false,
                italic: false,
                underline: false,
                inverse: false,
            }
    }
}

struct Palette {
    ansi: [Hsla; 16],
    fg: Hsla,
    bg: Hsla,
    /// `minimumContrastRatio`: coloured text is lightened or darkened until it reads this well.
    contrast: f32,
}

impl Palette {
    fn new(t: &Theme, contrast: f32) -> Self {
        let dark = t.bg_editor.l < 0.5;
        let ansi = if dark { ANSI_DARK } else { ANSI_LIGHT }.map(|c| rgb(c).into());
        Self {
            ansi,
            fg: t.text,
            bg: t.bg_editor,
            contrast,
        }
    }

    fn color(&self, color: vt100::Color, default: Hsla) -> Hsla {
        match color {
            vt100::Color::Default => default,
            vt100::Color::Idx(i) if i < 16 => self.ansi[i as usize],
            vt100::Color::Idx(i) if i < 232 => {
                let n = i - 16;
                let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v as u32 };
                rgb(level(n / 36) << 16 | level((n / 6) % 6) << 8 | level(n % 6)).into()
            }
            vt100::Color::Idx(i) => {
                let v = 8 + 10 * (i as u32 - 232);
                rgb(v << 16 | v << 8 | v).into()
            }
            vt100::Color::Rgb(r, g, b) => rgb((r as u32) << 16 | (g as u32) << 8 | b as u32).into(),
        }
    }

    fn style(&self, a: Attrs, bright_bold: bool, metrics: &Metrics) -> HighlightStyle {
        let fg_index = match a.fg {
            vt100::Color::Idx(i) if a.bold && bright_bold && i < 8 => vt100::Color::Idx(i + 8),
            fg => fg,
        };
        let (mut fg, mut bg) = (self.color(fg_index, self.fg), self.color(a.bg, self.bg));
        if a.inverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        if self.contrast > 1. {
            fg = ensure_contrast(fg, bg, self.contrast);
        }
        if a.dim {
            fg.a *= 0.5;
        }
        HighlightStyle {
            color: Some(fg),
            background_color: (a.inverse || a.bg != vt100::Color::Default).then_some(bg),
            font_weight: a.bold.then_some(metrics.bold),
            font_style: a.italic.then_some(FontStyle::Italic),
            underline: a.underline.then_some(UnderlineStyle {
                thickness: px(1.),
                color: Some(fg),
                wavy: false,
            }),
            ..Default::default()
        }
    }
}

/// One row of the screen as text, with its styled runs.
fn row_text(
    screen: &vt100::Screen,
    row: u16,
    cols: u16,
    palette: &Palette,
    bright_bold: bool,
    metrics: &Metrics,
) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let mut text = String::new();
    let mut runs: Vec<(Range<usize>, Attrs)> = Vec::new();
    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            break;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        let start = text.len();
        if cell.has_contents() {
            text.push_str(cell.contents());
        } else {
            text.push(' ');
        }
        let attrs = Attrs::of(cell);
        if attrs.plain() {
            continue;
        }
        match runs.last_mut() {
            Some((range, last)) if range.end == start && *last == attrs => range.end = text.len(),
            _ => runs.push((start..text.len(), attrs)),
        }
    }
    // Trailing blanks with nothing to show are left off.
    let kept = runs.last().map_or(0, |(r, _)| r.end);
    let trimmed = text.trim_end_matches(' ').len().max(kept);
    text.truncate(trimmed);
    let runs = runs
        .into_iter()
        .map(|(range, attrs)| (range, palette.style(attrs, bright_bold, metrics)))
        .collect();
    (text, runs)
}

pub(super) fn draw_screen(
    screen: &vt100::Screen,
    selection: Option<((u16, u16), (u16, u16))>,
    metrics: &Metrics,
    settings: &TerminalSettings,
    focused: bool,
    cursor_shown: bool,
    t: &Theme,
) -> impl IntoElement + use<> {
    let (rows, cols) = screen.size();
    let palette = Palette::new(t, settings.minimum_contrast_ratio);
    let (cell_w, line_h) = (metrics.cell_w, metrics.line_h);
    let lines = (0..rows).map(|row| {
        let (text, runs) = row_text(
            screen,
            row,
            cols,
            &palette,
            settings.draw_bold_text_in_bright_colors,
            metrics,
        );
        div()
            .h(px(line_h))
            .whitespace_nowrap()
            .child(StyledText::new(text).with_highlights(runs))
    });
    let selection = selection
        .map(ordered)
        .filter(|(a, b)| a != b)
        .map(|(a, b)| {
            (a.0..=b.0).map(move |row| {
                let from = if row == a.0 { a.1 } else { 0 };
                let to = if row == b.0 { b.1 } else { cols };
                div()
                    .absolute()
                    .top(px(row as f32 * line_h))
                    .left(px(from as f32 * cell_w))
                    .w(px(to.saturating_sub(from) as f32 * cell_w))
                    .h(px(line_h))
                    .bg(t.bg_selected)
            })
        });
    let cursor = (cursor_shown && !screen.hide_cursor() && screen.scrollback() == 0).then(|| {
        let (row, col) = screen.cursor_position();
        let under = screen
            .cell(row, col)
            .filter(|c| c.has_contents())
            .map(|c| c.contents().to_string())
            .unwrap_or_default();
        let at = div()
            .absolute()
            .top(px(row as f32 * line_h))
            .left(px(col as f32 * cell_w))
            .h(px(line_h));
        match settings.cursor_style {
            CursorStyle::Line => at.w(px(settings.cursor_width.max(1) as f32)).bg(t.text),
            CursorStyle::Underline => at.w(px(cell_w)).child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(2.))
                    .bg(t.text),
            ),
            CursorStyle::Block if focused => at
                .w(px(cell_w))
                .bg(t.text)
                .text_color(t.bg_editor)
                .child(under),
            CursorStyle::Block => at.w(px(cell_w)).border_1().border_color(t.text),
        }
    });
    div()
        .relative()
        .font_family(metrics.family.clone())
        .text_size(metrics.size)
        .line_height(px(line_h))
        .font_weight(metrics.weight)
        .text_color(palette.fg)
        .children(selection.into_iter().flatten())
        .child(div().flex().flex_col().children(lines))
        .children(cursor)
}
