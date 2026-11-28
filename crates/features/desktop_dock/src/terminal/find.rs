//! Find in the terminal, as the web terminal's search: a field in the panel's header that counts
//! the matches in the active shell's screen and the lines it keeps, marks them, and steps through
//! them with Enter, Shift+Enter or its arrows, scrolling the current one into view. Esc closes it.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Entity, Focusable, KeyDownEvent, Role, Stateful, Window, div, prelude::*, px};
use patchwork_design_editor::{Changed, Submitted, SubmittedBack, TextArea};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, fs_xs, radius_sm, theme};
use patchwork_design_components::{button, input_box};
use patchwork_design_icons::icon;

use super::select::{kept, top_line};
use crate::Dock;

/// The width of the find field, as the web terminal's `w-40`.
const FIELD_W: f32 = 160.;

/// One place the query was found: the line, counted from the oldest kept, and its columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Match {
    line: i64,
    from: u16,
    to: u16,
}

/// The find field and what it found.
#[derive(Default)]
pub(crate) struct FindState {
    field: Option<Entity<TextArea>>,
    pub(crate) open: bool,
    /// The shell the matches were found in, so another shell's are found when it shows.
    shell: Option<u32>,
    matches: Vec<Match>,
    current: usize,
}

impl std::fmt::Debug for FindState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FindState")
            .field("open", &self.open)
            .field("matches", &self.matches.len())
            .finish_non_exhaustive()
    }
}

/// Every match of `needle`, already lowercase, in the lines `screen` keeps and the rows it shows,
/// ignoring case. Lines are searched one at a time, so a match does not run across a wrap.
fn find_all(screen: &mut vt100::Screen, needle: &[char]) -> Vec<Match> {
    if needle.is_empty() {
        return Vec::new();
    }
    let offset = screen.scrollback();
    let kept = kept(screen) as i64;
    let (rows, cols) = screen.size();
    let mut found = Vec::new();
    let mut line = 0;
    // A view's worth of lines at a time, scrolled so the first is at the top while it is above
    // the screen.
    while line < kept + i64::from(rows) {
        let back = (kept - line).max(0);
        screen.set_scrollback(back as usize);
        let top = kept - back;
        let first = (line - top) as u16;
        for row in first..rows {
            find_in_row(screen, row, cols, needle, top + i64::from(row), &mut found);
        }
        line = top + i64::from(rows);
    }
    screen.set_scrollback(offset);
    found
}

/// The matches of `needle` in `row` of the view, pushed onto `found` as being on `line`.
fn find_in_row(
    screen: &vt100::Screen,
    row: u16,
    cols: u16,
    needle: &[char],
    line: i64,
    found: &mut Vec<Match>,
) {
    // The row's characters, lowercase, each with the column it starts at and its width.
    let mut chars: Vec<(char, u16, u16)> = Vec::new();
    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            break;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        let width = if cell.is_wide() { 2 } else { 1 };
        if cell.has_contents() {
            for ch in cell.contents().chars().flat_map(char::to_lowercase) {
                chars.push((ch, col, width));
            }
        } else {
            chars.push((' ', col, width));
        }
    }
    if chars.len() < needle.len() {
        return;
    }
    let mut at = 0;
    while at + needle.len() <= chars.len() {
        if chars[at..at + needle.len()]
            .iter()
            .zip(needle)
            .all(|((c, _, _), n)| c == n)
        {
            let (_, from, _) = chars[at];
            let (_, last, width) = chars[at + needle.len() - 1];
            found.push(Match {
                line,
                from,
                to: last + width,
            });
            at += needle.len();
        } else {
            at += 1;
        }
    }
}

impl Dock {
    fn find_query(&self, cx: &gpui::App) -> Vec<char> {
        self.find
            .field
            .as_ref()
            .map(|f| {
                f.read(cx)
                    .content
                    .chars()
                    .flat_map(char::to_lowercase)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Opens the find field, or focuses it when open, with its text selected so typing replaces
    /// the last search.
    pub(crate) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = if let Some(field) = &self.find.field {
            field.clone()
        } else {
            let field = cx.new(|cx| {
                let mut field = TextArea::new("Find", "Find in terminal", cx);
                field.single_line = true;
                field.placeholder_color = Some(patchwork_design_theme::input_placeholder());
                field
            });
            // The newest match first, since a terminal is read from its end.
            cx.subscribe(&field, |this, _, _: &Changed, cx| {
                this.refresh_find(cx);
                this.find.current = this.find.matches.len().saturating_sub(1);
                this.reveal_match();
                cx.notify();
            })
            .detach();
            cx.subscribe(&field, |this, _, _: &Submitted, cx| this.step_find(1, cx))
                .detach();
            cx.subscribe(&field, |this, _, _: &SubmittedBack, cx| {
                this.step_find(-1, cx);
            })
            .detach();
            self.find.field = Some(field.clone());
            field
        };
        self.find.open = true;
        self.menu = None;
        self.refresh_find(cx);
        window.focus(&field.read(cx).focus_handle(cx), cx);
        field.update(cx, |f, cx| f.select_everything(window, cx));
        cx.notify();
    }

    /// Closes the field, letting go of its marks, and gives the keys back to the terminal.
    pub(crate) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.open = false;
        self.find.matches.clear();
        window.focus(&self.term_focus, cx);
        cx.notify();
    }

    /// Finds the query again in the active shell, keeping the current match's place.
    pub(crate) fn refresh_find(&mut self, cx: &gpui::App) {
        let needle = self.find_query(cx);
        let id = self.active_shell().map(|s| s.id);
        self.find.shell = id;
        self.find.matches = match self.active_shell_mut() {
            Some(shell) if !needle.is_empty() => find_all(shell.parser.screen_mut(), &needle),
            _ => Vec::new(),
        };
        self.find.current = self
            .find
            .current
            .min(self.find.matches.len().saturating_sub(1));
    }

    /// Moves `delta` matches on, wrapping around, and scrolls the new one into view.
    fn step_find(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.refresh_find(cx);
        let count = self.find.matches.len();
        if count == 0 {
            return cx.notify();
        }
        let at = self.find.current.min(count - 1) as isize;
        self.find.current = (at + delta).rem_euclid(count as isize) as usize;
        self.reveal_match();
        cx.notify();
    }

    /// Scrolls the view through the lines kept so the current match is in it, in the middle
    /// when it was out of sight.
    fn reveal_match(&mut self) {
        let Some(m) = self.find.matches.get(self.find.current).copied() else {
            return;
        };
        let Some(shell) = self.active_shell_mut() else {
            return;
        };
        let screen = shell.parser.screen_mut();
        let rows = i64::from(screen.size().0);
        let top = top_line(screen);
        if (top..top + rows).contains(&m.line) {
            return;
        }
        let kept = kept(screen) as i64;
        let back = (kept - m.line + rows / 2).clamp(0, kept);
        screen.set_scrollback(back as usize);
    }

    /// The matches in the active shell's view, as the rows and columns to mark, and whether each
    /// is the current one. None while the field is closed.
    pub(crate) fn find_marks(&mut self, cx: &gpui::App) -> Vec<(u16, u16, u16, bool)> {
        if !self.find.open {
            return Vec::new();
        }
        if self.find.shell != self.active_shell().map(|s| s.id) {
            self.refresh_find(cx);
        }
        let current = self.find.matches.get(self.find.current).copied();
        let Some(shell) = self.active_shell_mut() else {
            return Vec::new();
        };
        let screen = shell.parser.screen_mut();
        let rows = i64::from(screen.size().0);
        let top = top_line(screen);
        self.find
            .matches
            .iter()
            .filter(|m| (top..top + rows).contains(&m.line))
            .map(|m| ((m.line - top) as u16, m.from, m.to, Some(*m) == current))
            .collect()
    }

    /// The find field with its count and arrows, while it is open.
    pub(crate) fn find_bar(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let field = self.find.field.clone().filter(|_| self.find.open)?;
        let t = *theme(cx);
        let focused = field.read(cx).focus_handle(cx).is_focused(window);
        let empty = self.find_query(cx).is_empty();
        let count = self.find.matches.len();
        let status = if empty {
            String::new()
        } else if count == 0 {
            "0".into()
        } else {
            format!("{}/{count}", self.find.current.min(count - 1) + 1)
        };
        Some(
            div()
                .id("terminal-find")
                .role(Role::Search)
                .aria_label("Find in terminal")
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .pr(px(4.))
                // Esc closes only the field, not the panel behind it.
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                    if e.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.close_find(window, cx);
                    }
                }))
                .child(
                    input_box(focused, &t)
                        .h(px(24.))
                        .w(px(FIELD_W))
                        .text_size(px(fs_xs()))
                        .child(div().flex_1().min_w_0().child(field)),
                )
                .child(
                    div()
                        .id("terminal-find-count")
                        .role(Role::Status)
                        .aria_label(status.clone())
                        .w(px(40.))
                        .flex_none()
                        .text_right()
                        .text_size(px(fs_xs()))
                        .text_color(t.text_muted)
                        .child(status),
                )
                .child(
                    small_button(
                        "terminal-find-previous",
                        "Previous match",
                        IconName::CaretUp,
                        &t,
                    )
                    .tooltip(patchwork_design_components::tooltip("Shift+Enter"))
                    .on_click(cx.listener(|this, _, _, cx| this.step_find(-1, cx))),
                )
                .child(
                    small_button("terminal-find-next", "Next match", IconName::CaretDown, &t)
                        .tooltip(patchwork_design_components::tooltip("Enter"))
                        .on_click(cx.listener(|this, _, _, cx| this.step_find(1, cx))),
                )
                .child(
                    small_button("terminal-find-close", "Close find", IconName::X, &t)
                        .tooltip(patchwork_design_components::tooltip("Esc"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))),
                )
                .into_any_element(),
        )
    }
}

/// A 20px icon button, as the web terminal's `icon-micro`.
fn small_button(
    id: &'static str,
    label: &'static str,
    name: IconName,
    t: &Theme,
) -> Stateful<gpui::Div> {
    let (fg, hover) = (t.text, t.bg_hover);
    button(id, label)
        .flex_none()
        .size(px(20.))
        .justify_center()
        .rounded(px(radius_sm()))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon(name, 12.))
}
