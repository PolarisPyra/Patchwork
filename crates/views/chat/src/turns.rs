//! A chat's turns: each message the person sent with the reply text after it, as the rail down
//! the transcript's edge lists them, and where each turn's strip sits.

// Rust guideline compliant 2026-10-09

use patchwork_model::chats::{Message, Role as MessageRole};
use patchwork_model::goal::{Upkeep, is_goal_continuation, upkeep};

/// The least room a strip keeps. Turns closer than this share a strip.
const PACKED: f32 = 6.;

/// A message the person sent, at its row, and the last reply text after it.
#[derive(Debug)]
pub struct Turn<'a> {
    pub index: usize,
    pub ask: &'a str,
    pub answer: Option<&'a str>,
    /// The first turn of a day, or the first after a `/compact`.
    pub mark: bool,
}

/// A `/compact` is a divider, and a goal's pause, clear or continuation is not shown, so none
/// starts a turn.
#[must_use]
pub fn starts_turn(m: &Message) -> bool {
    m.role == MessageRole::User && upkeep(&m.text).is_none() && !is_goal_continuation(&m.text)
}

/// A goal message the chat does not show: a `/goal` command Patchwork answers itself, such as a
/// pause or a clear, or the message Patchwork sends to carry the goal on.
#[must_use]
pub fn is_goal_upkeep(text: &str) -> bool {
    upkeep(text) == Some(Upkeep::Goal) || is_goal_continuation(text)
}

/// The turns among `rows`, in order, each with the last reply text that answers it.
#[must_use]
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
                });
                compacted = false;
                day = today.or(day);
            } else if upkeep(&m.text) == Some(Upkeep::Compact) {
                compacted = true;
            }
        } else if answering
            && !m.text.is_empty()
            && let Some(turn) = turns.last_mut()
        {
            turn.answer = Some(&m.text);
        }
    }
    turns
}

/// The local day a time falls on, counted in days.
#[must_use]
pub fn day_of(ms: u64) -> Option<i32> {
    use chrono::{Datelike, Local, TimeZone};
    let time = Local
        .timestamp_millis_opt(i64::try_from(ms).ok()?)
        .single()?;
    Some(time.num_days_from_ce())
}

/// One line of text for a preview, with Markdown's marks taken out.
#[must_use]
pub fn preview_text(text: &str) -> Option<String> {
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

/// Where each turn's strip sits down a rail of `height`, on whole pixels. Turns with less than
/// [`PACKED`] between them land on the same pixel, and so share a strip.
#[must_use]
pub fn strip_positions(count: usize, height: f32) -> Vec<f32> {
    let steps = count.saturating_sub(1);
    let slots = steps.min((height / PACKED) as usize);
    (0..count)
        .map(|i| {
            if slots == 0 {
                return 0.;
            }
            let slot = (i as f32 * slots as f32 / steps as f32).round();
            (slot / slots as f32 * height).round()
        })
        .collect()
}
