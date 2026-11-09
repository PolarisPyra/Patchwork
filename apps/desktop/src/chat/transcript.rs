//! The transcript: the open chat's messages in a list that draws only what is in view.

use gpui::{
    AnyElement, Context, Div, FollowMode, IntoElement, Role, SharedString, Window, div, list,
    prelude::*, px,
};
use patchwork_server::threads::{Message, MessageStatus, Part, Role as MessageRole, Thread};

use crate::theme::{FS_XS, Theme};
use crate::ui::{format_tokens, list_scrollbar, spinner};
use crate::workspace::{Workspace, is_busy};

use super::markdown::find_in;
use super::reply::{pieces_of, reply_piece};
use super::timeline::{Step, steps_of};
use super::{rail, user};

/// What one item of the transcript's list draws. A reply is cut into pieces, so a frame lays out
/// only the pieces in view, not every step of a long reply.
#[derive(Clone, Copy, PartialEq)]
pub enum Piece {
    /// A whole message: the user's, a compacted divider, or a reply that only clears the goal.
    Whole,
    /// A reply's head line: "Working for 12s", or its fold.
    Head,
    /// A reply's step `step`; of a group of calls, calls `from..to`; of a long text, its bytes
    /// `from..to`.
    Step { step: usize, from: usize, to: usize },
    /// What sits under a reply's steps: its files, notices and actions.
    Foot,
}

/// One item of the transcript's list: a piece of the shown message at `message`.
#[derive(Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub message: usize,
    pub piece: Piece,
}

/// Keeps the transcript's list in step with the open chat: another chat starts over at its end,
/// items that arrive or go are spliced in, and the last item and the last piece of any reply still
/// streaming are measured again, since they can grow out of sight.
pub fn sync_transcript(w: &mut Workspace) {
    let Some(thread) = &w.thread else {
        return;
    };
    let mut rows = Vec::new();
    for (i, m) in thread.messages.iter().filter(|m| !m.queued).enumerate() {
        let pieces = if m.role == MessageRole::User {
            vec![Piece::Whole]
        } else {
            pieces_of(w, m)
        };
        rows.extend(pieces.into_iter().map(|piece| Row {
            id: m.id.clone(),
            message: i,
            piece,
        }));
    }
    let list = &w.transcript_list;
    let (shown_in, old) = &w.transcript_rows;
    if shown_in.as_deref() != Some(thread.id.as_str()) {
        list.reset(rows.len());
        list.set_follow_mode(FollowMode::Tail);
    } else if *old == rows {
        let streaming = |i: usize| {
            thread
                .messages
                .iter()
                .find(|m| m.id == rows[i].id)
                .is_some_and(|m| m.status == MessageStatus::Streaming)
        };
        for i in 0..rows.len() {
            let last_piece = rows.get(i + 1).is_none_or(|next| next.id != rows[i].id);
            if last_piece && (i + 1 == rows.len() || streaming(i)) {
                list.remeasure_items(i..i + 1);
            }
        }
        return;
    } else {
        let same = old.iter().zip(&rows).take_while(|(a, b)| a == b).count();
        let tail = old[same..]
            .iter()
            .rev()
            .zip(rows[same..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        list.splice(same..old.len() - tail, rows.len() - same - tail);
    }
    w.transcript_rows = (Some(thread.id.clone()), rows);
}

impl Workspace {
    /// The first item of the shown message at `message` in the transcript's list.
    pub fn item_of(&self, message: usize) -> usize {
        self.transcript_rows
            .1
            .iter()
            .position(|r| r.message == message)
            .unwrap_or(message)
    }

    /// The item that draws match `nth` of `needle` in the shown message at `message`: the piece
    /// with its text, or the message's first piece when that text is folded away.
    pub fn item_of_match(&self, message: usize, nth: usize, needle: &str) -> usize {
        let first = self.item_of(message);
        let Some(m) = self
            .thread
            .as_ref()
            .and_then(|t| t.messages.iter().filter(|m| !m.queued).nth(message))
        else {
            return first;
        };
        if m.role == MessageRole::User {
            return first;
        }
        let mut seen = 0;
        for (i, step) in steps_of(m).iter().enumerate() {
            if let Step::Text(text) = step {
                let found = find_in(text, needle);
                if let Some(&at) = found.get(nth - seen) {
                    return self
                        .transcript_rows
                        .1
                        .iter()
                        .position(|r| {
                            r.message == message
                                && matches!(r.piece, Piece::Step { step, from, to }
                                    if step == i && (from..to).contains(&at))
                        })
                        .unwrap_or(first);
                }
                seen += found.len();
            }
        }
        first
    }
}

/// Where a `/compact` is: running, done, or stopped before it was.
#[derive(PartialEq)]
enum Compaction {
    Compacting,
    Compacted,
    Stopped,
}

/// The state of the `/compact` at `index`, read from the reply after it.
fn compaction_state(rows: &[&Message], index: usize, busy: bool) -> Compaction {
    let reply = rows.get(index + 1);
    // A /compact sent during a turn compacts where the turn is, and the turn's reply streams on.
    if reply.is_some_and(|r| r.parts.iter().any(|p| matches!(p, Part::Compaction { .. }))) {
        return Compaction::Compacted;
    }
    match reply {
        Some(r) if r.status == MessageStatus::Streaming => Compaction::Compacting,
        None if busy => Compaction::Compacting,
        None => Compaction::Stopped,
        Some(r) if matches!(r.status, MessageStatus::Error | MessageStatus::Cancelled) => {
            Compaction::Stopped
        }
        Some(_) => Compaction::Compacted,
    }
}

fn compacted_divider(
    row: SharedString,
    state: Compaction,
    before: Option<u64>,
    t: &Theme,
) -> gpui::Stateful<Div> {
    let rule = || div().h(px(1.)).flex_1().bg(t.border);
    let label = match state {
        Compaction::Compacting => div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(spinner(11., t.text_subtle))
            .child("Compacting context"),
        Compaction::Stopped => div().child("Compaction stopped"),
        Compaction::Compacted => div().child(match before {
            Some(was) => format!("Context compacted · was {} tokens", format_tokens(was)),
            None => "Context compacted".into(),
        }),
    };
    div().id(row).px(px(24.)).child(
        div().flex().pb(px(20.)).child(
            div()
                .flex()
                .w_full()
                .items_center()
                .gap(px(12.))
                .text_size(px(FS_XS))
                .text_color(t.text_subtle)
                .child(rule())
                .child(label)
                .child(rule()),
        ),
    )
}

pub fn transcript(
    w: &Workspace,
    thread: &Thread,
    t: &Theme,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Div {
    // One still queued is waiting above the composer, not a row here.
    let shown: Vec<&Message> = thread.messages.iter().filter(|m| !m.queued).collect();
    let rail = rail::turn_rail(w, &shown, t, window, cx);
    let marks = w.find_marks(cx);
    let busy = is_busy(thread);
    // The context size in force just before each row, for a compacted divider to compare against.
    let mut latest = None;
    let before: Vec<Option<u64>> = shown
        .iter()
        .map(|m| {
            let was = latest;
            if let Some(usage) = &m.usage {
                latest = Some(usage.context);
            }
            was
        })
        .collect();
    let theme = *t;
    let rows = list(
        w.transcript_list.clone(),
        cx.processor(move |w, i: usize, _, cx| {
            transcript_row(w, i, &before, &marks, busy, &theme, cx)
        }),
    )
    .flex_1()
    .min_h_0();
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(
            div()
                .id("transcript")
                .role(Role::Log)
                .aria_label("Transcript")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(rows),
        )
        .children(list_scrollbar(&w.transcript_list, 4., 0., t.border_strong))
        .children(rail)
}

/// Row `i` of the open chat's transcript, drawn once the list scrolls it into reach.
fn transcript_row(
    w: &Workspace,
    i: usize,
    before: &[Option<u64>],
    marks: &Option<(String, Option<(String, usize)>)>,
    busy: bool,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let Some(thread) = &w.thread else {
        return gpui::Empty.into_any_element();
    };
    let rows = &w.transcript_rows.1;
    let Some(&Row { message, piece, .. }) = rows.get(i) else {
        return gpui::Empty.into_any_element();
    };
    let shown: Vec<&Message> = thread.messages.iter().filter(|m| !m.queued).collect();
    let Some(&m) = shown.get(message) else {
        return gpui::Empty.into_any_element();
    };
    let first = i == 0 || rows[i - 1].message != message;
    let last = rows.get(i + 1).is_none_or(|next| next.message != message);
    let row = SharedString::from(match piece {
        Piece::Whole => format!("row-{}", m.id),
        Piece::Head => format!("row-{}-head", m.id),
        Piece::Step { step, from, .. } => format!("row-{}-{step}-{from}", m.id),
        Piece::Foot => format!("row-{}-foot", m.id),
    });
    if m.role == MessageRole::User && rail::is_goal_clear(&m.text) {
        // The green notice in the reply already says the goal was cleared.
        return div().id(row).into_any_element();
    }
    if m.role == MessageRole::User && m.text.trim() == "/compact" {
        let was = before
            .get(message)
            .copied()
            .flatten()
            .filter(|_| w.settings.tokens.show_usage);
        return compacted_divider(row, compaction_state(&shown, message, busy), was, t)
            .into_any_element();
    }
    let previous = message.checked_sub(1).and_then(|p| shown.get(p).copied());
    let find = w.find_in_row(marks, &m.id);
    let body = if m.role == MessageRole::User {
        user::user_message(w, m, find.as_ref(), t, cx).into_any_element()
    } else {
        reply_piece(w, thread, m, previous, piece, find.as_ref(), t, cx).into_any_element()
    };
    // A reply's first piece stands for the whole reply.
    let label = match (m.role == MessageRole::User, first) {
        (true, _) => Some("Your message"),
        (false, true) => Some("Reply"),
        (false, false) => None,
    };
    div()
        .id(row)
        .when_some(label, |d, label| d.role(Role::Article).aria_label(label))
        .px(px(24.))
        .child(
            div()
                .flex()
                .flex_col()
                .when(last, |d| d.pb(px(20.)))
                .when(first && message == 0, |d| d.pt(px(24.)))
                .when(m.role != MessageRole::User, |d| d.max_w(px(820.)))
                .child(body),
        )
        .into_any_element()
}
