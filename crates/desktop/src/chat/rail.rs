//! A strip per turn down the transcript's right edge. The
//! strips of the turns on screen are lit, the ones near the pointer widen, and the turn under it
//! shows a preview of its message and reply. Clicking a strip, or choosing one with the arrow keys
//! and Enter, scrolls to that turn.

// Rust guideline compliant 2026-10-09

use gpui::{
    App, Div, FocusHandle, FontWeight, KeyDownEvent, ListOffset, Pixels, Window, div, prelude::*,
    px,
};
use patchwork_server::threads::{Message, Role as MessageRole};

use crate::ui::theme::{FS_LG, FS_MD, RADIUS_LG, Theme};
use crate::ui::{button, popup_shadow};
use crate::workspace::Workspace;

/// Pixels between two strips, until the rail would pass its maximum height.
const SPACING: f32 = 12.;
/// Fewer turns than this need no rail.
const MIN_TURNS: usize = 2;
/// Room the rail leaves above and below it.
const ROOM: f32 = 128.;
/// How far below the transcript's top a chosen turn lands.
const VIEW_OFFSET: f32 = 24.;

/// A message the person sent, at its row, and the last reply text after it.
pub struct Turn {
    index: usize,
    ask: String,
    answer: Option<String>,
}

/// The turn under the pointer, and the one chosen with the keys while the rail has focus.
pub struct RailState {
    hover: Option<usize>,
    key: Option<usize>,
    focus: FocusHandle,
}

impl RailState {
    pub fn new(cx: &mut App) -> Self {
        Self {
            hover: None,
            key: None,
            focus: cx.focus_handle(),
        }
    }
}

/// A `/compact` is a divider and a `/goal clear` is not shown, so neither starts a turn.
pub fn starts_turn(m: &Message) -> bool {
    let text = m.text.trim();
    m.role == MessageRole::User && text != "/compact" && !is_goal_clear(text)
}

/// `/goal clear` or one of its aliases, as the backend reads them.
pub fn is_goal_clear(text: &str) -> bool {
    let mut words = text.split_whitespace();
    words.next() == Some("/goal")
        && words.next().is_some_and(|w| {
            ["clear", "stop", "off", "reset", "none", "cancel"]
                .iter()
                .any(|alias| w.eq_ignore_ascii_case(alias))
        })
        && words.next().is_none()
}

pub fn turns_of(rows: &[&Message]) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    for (index, m) in rows.iter().enumerate() {
        if starts_turn(m) {
            turns.push(Turn {
                index,
                ask: m.text.clone(),
                answer: None,
            });
        } else if m.role != MessageRole::User
            && !m.text.is_empty()
            && let Some(turn) = turns.last_mut()
        {
            turn.answer = Some(m.text.clone());
        }
    }
    turns
}

/// One line of text for a preview, with Markdown's marks taken out.
fn preview_text(text: &str) -> Option<String> {
    let text: String = text.chars().take(1_500).collect();
    // Code blocks go whole, an unclosed one to the end.
    let mut plain = String::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("```") {
        plain.push_str(&rest[..start]);
        plain.push(' ');
        rest = match rest[start + 3..].find("```") {
            Some(end) => &rest[start + 3 + end + 3..],
            None => "",
        };
    }
    plain.push_str(rest);
    // A link or image keeps only its text.
    let mut unlinked = String::new();
    let mut rest = plain.as_str();
    while let Some(open) = rest.find('[') {
        let label_end = rest[open..].find("](").map(|i| open + i);
        let close = label_end.and_then(|l| rest[l..].find(')').map(|i| l + i));
        match (label_end, close) {
            (Some(label_end), Some(close)) if !rest[open + 1..label_end].contains(']') => {
                unlinked.push_str(rest[..open].trim_end_matches('!'));
                unlinked.push_str(&rest[open + 1..label_end]);
                rest = &rest[close + 1..];
            }
            _ => {
                unlinked.push_str(&rest[..=open]);
                rest = &rest[open + 1..];
            }
        }
    }
    unlinked.push_str(rest);
    let words: Vec<&str> = unlinked
        .split(|c: char| c.is_whitespace() || "`*_~#>|".contains(c))
        .filter(|w| !w.is_empty())
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// How far down a rail of `height` turn `index` of `count` sits.
fn top_of(index: usize, count: usize, height: f32) -> f32 {
    if count <= 1 {
        0.
    } else {
        index as f32 / (count - 1) as f32 * height
    }
}

impl Workspace {
    /// Scrolls the transcript so the turn's row sits just below its top.
    fn jump_to_row(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.transcript_list.scroll_to(ListOffset {
            item_ix: self.item_of(row),
            offset_in_item: px(0.),
        });
        // The gap above the turn comes out of the row before it, which the list measures only
        // once the frame after this one has laid out around the turn.
        cx.on_next_frame(window, |_, window, cx| {
            cx.on_next_frame(window, |w, _, cx| {
                w.transcript_list.scroll_by(px(-VIEW_OFFSET));
                cx.notify();
            });
        });
        cx.notify();
    }

    /// Clears the turn chosen with the keys once the rail loses focus.
    pub fn watch_rail_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.on_focus_out(&self.rail.focus, window, |w, _, _, cx| {
            w.rail.key = None;
            cx.notify();
        })
        .detach();
    }
}

/// The rows that reach into the transcript's view, first and last, from its last layout.
fn rows_in_view(w: &Workspace) -> Option<(usize, usize)> {
    let list = &w.transcript_list;
    let view = list.viewport_bounds();
    let mut seen: Option<(usize, usize)> = None;
    let items = &w.transcript_rows.1;
    for (item, at) in items
        .iter()
        .enumerate()
        .skip(list.logical_scroll_top().item_ix)
    {
        let row = at.message;
        let Some(bounds) = list.bounds_for_item(item) else {
            break;
        };
        let top = bounds.top();
        if top >= view.bottom() {
            break;
        }
        if top + bounds.size.height.max(px(1.)) > view.top() {
            seen = Some((seen.map_or(row, |(first, _)| first), row));
        }
    }
    seen
}

pub fn turn_rail(
    w: &Workspace,
    rows: &[&Message],
    t: &Theme,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Option<Div> {
    let turns = turns_of(rows);
    let count = turns.len();
    let view: Pixels = w.transcript_list.viewport_bounds().size.height;
    let view = f32::from(view);
    if count < MIN_TURNS || view <= ROOM {
        return None;
    }
    let height = ((count - 1) as f32 * SPACING).min(view - ROOM);
    let top = (view - height) / 2.;
    let seen = rows_in_view(w);

    // The first turn that starts on screen, or else the one being read above it.
    let mut current = None;
    if let Some((first, last)) = seen {
        for (n, turn) in turns.iter().enumerate() {
            if turn.index >= first {
                if turn.index <= last {
                    current = Some(n);
                }
                break;
            }
            current = Some(n);
        }
    }
    let focused = w.rail.focus.is_focused(window);
    let shown = w
        .rail
        .hover
        .or(w.rail.key.filter(|_| focused))
        .or(current.filter(|_| focused))
        .filter(|&i| i < count);
    let named = shown.or(current).unwrap_or(0);
    let label = format!(
        "Jump to turn {} of {count}: {}",
        named + 1,
        preview_text(&turns[named].ask).unwrap_or_else(|| "message".into())
    );

    let strips = turns.iter().enumerate().map(|(index, turn)| {
        let distance = shown.map(|s| s.abs_diff(index));
        let scale = match distance {
            Some(0) => 1.,
            Some(1) => 0.667,
            Some(2) => 0.417,
            _ => 0.333,
        };
        let lit = seen.is_some_and(|(first, last)| turn.index >= first && turn.index <= last);
        div()
            .absolute()
            .right_0()
            .top(px(top_of(index, count, height) - 1.5))
            .w(px(32. * scale))
            .h(px(3.))
            .rounded_full()
            .bg(if distance == Some(0) {
                t.text_muted
            } else {
                t.text_subtle
            })
            .when(lit, |s| {
                s.child(div().size_full().rounded_full().bg(t.text.opacity(0.9)))
            })
    });

    let preview = shown.map(|index| {
        let turn = &turns[index];
        let y = top_of(index, count, height);
        div()
            .absolute()
            .right(px(40.))
            .top(px(y))
            .h(px(0.))
            .flex()
            .flex_col()
            .map(|d| {
                if index == 0 {
                    d.justify_start()
                } else if index == count - 1 {
                    d.justify_end()
                } else {
                    d.justify_center()
                }
            })
            .child(
                div()
                    .flex_none()
                    .w(px(384.))
                    .rounded(px(RADIUS_LG))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.bg_elevated)
                    .px(px(16.))
                    .py(px(12.))
                    .shadow(popup_shadow(t))
                    .child(
                        div()
                            .line_clamp(2)
                            .text_size(px(FS_LG))
                            .font_weight(FontWeight::MEDIUM)
                            .line_height(gpui::relative(1.375))
                            .text_color(t.text)
                            .child(preview_text(&turn.ask).unwrap_or_else(|| "Message".into())),
                    )
                    .children(turn.answer.as_deref().and_then(preview_text).map(|answer| {
                        div()
                            .mt(px(6.))
                            .line_clamp(4)
                            .text_size(px(FS_MD))
                            .line_height(gpui::relative(1.625))
                            .text_color(t.text_muted)
                            .child(answer)
                    })),
            )
    });

    let rail_top = w.transcript_list.viewport_bounds().top() + px(top);
    let index_at = move |y: Pixels| {
        let progress = (f32::from(y - rail_top) / height.max(1.)).clamp(0., 1.);
        (progress * (count - 1) as f32).round() as usize
    };
    let rows_click: Vec<usize> = turns.iter().map(|turn| turn.index).collect();
    Some(
        div()
            .absolute()
            .top(px(top))
            .right(px(12.))
            .w(px(32.))
            .h(px(height))
            .child(
                button("turn-rail", label)
                    .absolute()
                    .inset_0()
                    .track_focus(&w.rail.focus)
                    .on_mouse_move(cx.listener(move |w, e: &gpui::MouseMoveEvent, _, cx| {
                        let index = Some(index_at(e.position.y));
                        if w.rail.hover != index {
                            w.rail.hover = index;
                            cx.notify();
                        }
                    }))
                    .on_hover(cx.listener(|w, hovered: &bool, _, cx| {
                        if !hovered {
                            w.rail.hover = None;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |w, e: &gpui::ClickEvent, window, cx| {
                        // A click from the keyboard has no pointer position, so it takes the turn
                        // chosen.
                        let index = if e.is_keyboard() {
                            w.rail.key.or(current)
                        } else {
                            Some(index_at(e.position().y))
                        };
                        if let Some(&row) = index.and_then(|i| rows_click.get(i)) {
                            w.jump_to_row(row, window, cx);
                        }
                    }))
                    .on_key_down(cx.listener(move |w, e: &KeyDownEvent, _, cx| {
                        let last = count - 1;
                        let from = w.rail.key.or(current).unwrap_or(0);
                        w.rail.key = Some(match e.keystroke.key.as_str() {
                            "down" => (from + 1).min(last),
                            "up" => from.saturating_sub(1),
                            "home" => 0,
                            "end" => last,
                            _ => return,
                        });
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .children(strips),
            )
            .children(preview),
    )
}
