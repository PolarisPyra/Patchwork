//! A strip per turn down the transcript's right edge. The
//! strips of the turns on screen are lit, the ones near the pointer widen, and the turn under it
//! shows a preview of its message and reply. Clicking a strip, or choosing one with the arrow keys
//! and Enter, scrolls to that turn.
//!
//! A chat with more turns than the rail has room for packs its strips thin and spreads out only
//! the few around the pointer, or around the turn being read. A day's first turn and the first
//! turn after a `/compact` have a long strip, and a turn that edited files a longer one than the
//! rest, so a long rail has a shape to find a place by.

// Rust guideline compliant 2026-10-09

use std::rc::Rc;

use gpui::{
    App, Div, FocusHandle, FontWeight, KeyDownEvent, ListOffset, Pixels, Role, Window, div,
    prelude::*, px,
};
use patchwork_core::orchestration::{Upkeep, upkeep};
use patchwork_core::threads::{Message, Role as MessageRole};

use crate::ui::theme::{fs_lg, fs_md, radius_lg, Theme};
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
/// How many turns on each side of the one in the middle a crowded rail spreads out.
const LENS: usize = 4;
/// The least room a strip outside that span keeps.
const MIN_PITCH: f32 = 1.;

/// A message the person sent, at its row, and the last reply text after it.
pub struct Turn<'a> {
    index: usize,
    ask: &'a str,
    answer: Option<&'a str>,
    /// The first turn of a day, or the first after a `/compact`.
    mark: bool,
    /// The agent edited files in it.
    edited: bool,
}

/// The turn under the pointer, and the one chosen with the keys while the rail has focus.
pub struct RailState {
    hover: Option<usize>,
    /// On a crowded rail, the turn the pointer spread the strips around and where it sits. It
    /// stays put while the pointer is among those strips, so each is a full step apart.
    lens: Option<(usize, f32)>,
    key: Option<usize>,
    focus: FocusHandle,
}

impl RailState {
    pub fn new(cx: &mut App) -> Self {
        Self {
            hover: None,
            lens: None,
            key: None,
            focus: cx.focus_handle(),
        }
    }

    /// Forgets the turn under the pointer and the one chosen, which belong to the chat just left.
    pub fn clear(&mut self) {
        self.hover = None;
        self.lens = None;
        self.key = None;
    }
}

/// A `/compact` is a divider and a goal clear is not shown, so neither starts a turn.
pub fn starts_turn(m: &Message) -> bool {
    m.role == MessageRole::User && upkeep(&m.text).is_none()
}

/// A message that has the agent drop its goal, in any of the forms the core sends or reads.
pub fn is_goal_clear(text: &str) -> bool {
    upkeep(text) == Some(Upkeep::GoalClear)
}

/// Whether the chat has a rail: turns enough, and a transcript tall enough to hold one. The
/// transcript leaves room for it from this. A list just handed over has no height yet, which
/// counts as tall enough, so the room stays while the rail skips that frame.
pub fn has_rail(w: &Workspace, rows: &[&Message]) -> bool {
    let view = f32::from(w.transcript_list.viewport_bounds().size.height);
    (view == 0. || view > ROOM)
        && rows
            .iter()
            .filter(|m| starts_turn(m))
            .nth(MIN_TURNS - 1)
            .is_some()
}

pub fn turns_of<'a>(rows: &[&'a Message]) -> Vec<Turn<'a>> {
    let mut turns: Vec<Turn> = Vec::new();
    // A reply to a `/compact` or a goal clear answers that, not the turn before it.
    let mut answering = false;
    let mut compacted = false;
    let mut day = None;
    for (index, m) in rows.iter().enumerate() {
        if m.role == MessageRole::User {
            answering = starts_turn(m);
            if answering {
                let today = day_of(m.created_at);
                turns.push(Turn {
                    index,
                    ask: &m.text,
                    answer: None,
                    mark: compacted || day.is_some_and(|day| Some(day) != today),
                    edited: false,
                });
                compacted = false;
                day = today.or(day);
            } else if upkeep(&m.text) == Some(Upkeep::Compact) {
                compacted = true;
            }
        } else if answering && let Some(turn) = turns.last_mut() {
            turn.edited |= !m.files.is_empty();
            if !m.text.is_empty() {
                turn.answer = Some(&m.text);
            }
        }
    }
    turns
}

/// The local day a time falls on, counted in days.
fn day_of(ms: u64) -> Option<i32> {
    use chrono::{Datelike, Local, TimeZone};
    let time = Local
        .timestamp_millis_opt(i64::try_from(ms).ok()?)
        .single()?;
    Some(time.num_days_from_ce())
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
    // Emphasis and code marks come off a word's ends, and a word of only heading, quote or table
    // marks goes, so `follow_width` and #12 stay as written.
    let words: Vec<&str> = unlinked
        .split_whitespace()
        .map(|w| w.trim_matches(|c| "`*_~".contains(c)))
        .filter(|w| !w.chars().all(|c| "#>|-".contains(c)))
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

/// Where each strip sits down a rail of `height`, and the span that is spread out. `lens` names
/// the turn in the middle of that span and where it sits; a rail with room for every turn, or
/// none for the span, spaces them evenly and has no span.
fn layout(count: usize, height: f32, lens: Option<(usize, f32)>) -> (Vec<f32>, Option<(f32, f32)>) {
    let last = count.saturating_sub(1);
    let even = || (0..count).map(|i| top_of(i, count, height)).collect();
    let Some((mid, at)) = lens.filter(|&(mid, _)| mid <= last && height < last as f32 * SPACING)
    else {
        return (even(), None);
    };
    let (above, below) = (mid.min(LENS), (last - mid).min(LENS));
    let (first, end_turn) = (mid - above, mid + below);
    let low = above as f32 * SPACING + first as f32 * MIN_PITCH;
    let high = height - below as f32 * SPACING - (last - end_turn) as f32 * MIN_PITCH;
    if low > high {
        return (even(), None);
    }
    let at = at.clamp(low, high);
    let (start, end) = (at - above as f32 * SPACING, at + below as f32 * SPACING);
    let tops = (0..count)
        .map(|i| {
            if i < first {
                i as f32 / first as f32 * start
            } else if i <= end_turn {
                start + (i - first) as f32 * SPACING
            } else {
                end + (i - end_turn) as f32 / (last - end_turn) as f32 * (height - end)
            }
        })
        .collect();
    (tops, Some((start, end)))
}

/// The strip closest to `y`, of strips listed from the top down.
fn nearest(tops: &[f32], y: f32) -> usize {
    let after = tops.partition_point(|&top| top < y);
    match (after.checked_sub(1), tops.get(after)) {
        (Some(before), Some(&next)) if y - tops[before] <= next - y => before,
        (Some(before), None) => before,
        _ => after,
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
    let view = f32::from(w.transcript_list.viewport_bounds().size.height);
    // A list just handed over has no height until it is laid out.
    if !has_rail(w, rows) || view == 0. {
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
    // The strips spread around the pointer, or else around the turn chosen or being read.
    let held = w.rail.hover.is_some() && w.rail.lens.is_some();
    let lens = w.rail.lens.filter(|_| held).or_else(|| {
        let turn = w.rail.key.filter(|_| focused).or(current)?;
        Some((turn, top_of(turn, count, height)))
    });
    let (tops, zone) = layout(count, height, lens);
    let tops: Rc<[f32]> = tops.into();
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
        preview_text(turns[named].ask).unwrap_or_else(|| "message".into())
    );

    let strips = turns.iter().enumerate().map(|(index, turn)| {
        let distance = shown.map(|s| s.abs_diff(index));
        let near: f32 = match distance {
            Some(0) => 1.,
            Some(1) => 0.667,
            Some(2) => 0.417,
            _ => 0.333,
        };
        let scale = near.max(if turn.mark {
            0.6
        } else if turn.edited {
            0.45
        } else {
            0.
        });
        // A strip is no thicker than half the room it has, so packed strips stay apart.
        let room = [index.checked_sub(1), Some(index + 1)]
            .into_iter()
            .flatten()
            .filter_map(|other| tops.get(other))
            .map(|other| (other - tops[index]).abs())
            .fold(SPACING, f32::min);
        let thick = (room / 2.).clamp(1., 3.);
        let lit = seen.is_some_and(|(first, last)| turn.index >= first && turn.index <= last);
        div()
            .absolute()
            .right_0()
            .top(px(tops[index] - thick / 2.))
            .w(px(32. * scale))
            .h(px(thick))
            .rounded_full()
            .bg(if distance == Some(0) || turn.mark {
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
        let y = tops[index];
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
                    .rounded(px(radius_lg()))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.bg_elevated)
                    .px(px(16.))
                    .py(px(12.))
                    .shadow(popup_shadow(t))
                    .child(
                        div()
                            .line_clamp(2)
                            .text_size(px(fs_lg()))
                            .font_weight(FontWeight::MEDIUM)
                            .line_height(gpui::relative(1.375))
                            .text_color(t.text)
                            .child(preview_text(turn.ask).unwrap_or_else(|| "Message".into())),
                    )
                    .children(turn.answer.and_then(preview_text).map(|answer| {
                        div()
                            .mt(px(6.))
                            .line_clamp(4)
                            .text_size(px(fs_md()))
                            .line_height(gpui::relative(1.625))
                            .text_color(t.text_muted)
                            .child(answer)
                    })),
            )
    });

    let rail_top = w.transcript_list.viewport_bounds().top() + px(top);
    let down = move |y: Pixels| f32::from(y - rail_top);
    let (tops_move, tops_click) = (Rc::clone(&tops), Rc::clone(&tops));
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
                    // The arrow keys move through the turns, as a slider's do through its values.
                    .role(Role::Slider)
                    .aria_orientation(gpui::Orientation::Vertical)
                    .aria_numeric_value((named + 1) as f64)
                    .aria_min_numeric_value(1.)
                    .aria_max_numeric_value(count as f64)
                    .track_focus(&w.rail.focus)
                    .on_mouse_move(cx.listener(move |w, e: &gpui::MouseMoveEvent, _, cx| {
                        let y = down(e.position.y);
                        let among = zone.is_none_or(|(start, end)| {
                            held && y >= start - SPACING / 2. && y <= end + SPACING / 2.
                        });
                        // Among the strips spread out, the pointer takes the closest. Past them
                        // it takes the turn that far down the chat, and they spread around it.
                        let (index, lens) = if among {
                            (nearest(&tops_move, y), w.rail.lens)
                        } else {
                            let progress = (y / height.max(1.)).clamp(0., 1.);
                            let index = (progress * (count - 1) as f32).round() as usize;
                            (index, Some((index, y)))
                        };
                        if w.rail.hover != Some(index) || w.rail.lens != lens {
                            w.rail.hover = Some(index);
                            w.rail.lens = lens;
                            cx.notify();
                        }
                    }))
                    .on_hover(cx.listener(|w, hovered: &bool, _, cx| {
                        if !hovered {
                            w.rail.hover = None;
                            w.rail.lens = None;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |w, e: &gpui::ClickEvent, window, cx| {
                        // A click from the keyboard has no pointer position, so it takes the turn
                        // chosen.
                        let index = if e.is_keyboard() {
                            w.rail.key.or(current)
                        } else {
                            w.rail
                                .hover
                                .or_else(|| Some(nearest(&tops_click, down(e.position().y))))
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
