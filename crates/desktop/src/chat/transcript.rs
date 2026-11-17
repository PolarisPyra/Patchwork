//! The transcript: the open chat's messages in a list that draws only what is in view.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, Div, FollowMode, ListAlignment, ListState, Role, SharedString, Window, div, list,
    prelude::*, px,
};
use patchwork_core::orchestration::{Upkeep, upkeep};
use patchwork_core::threads::{Message, MessageStatus, Part, Role as MessageRole, Thread};

use crate::ui::theme::{fs_xs, Theme};
use crate::ui::{format_tokens, list_scrollbar, spinner};
use crate::workspace::{Workspace, is_busy};

use crate::ui::markdown::find_in;
use super::reply::{is_open, pieces_of, reply_piece};
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

/// The pieces a shown message was last cut into, and what they were cut from.
pub struct Pieces {
    id: String,
    /// The message they were cut from, kept so that one changed since is a different allocation.
    /// A reply still streaming keeps none: it changes in place, and is cut again every frame.
    of: Option<Arc<Message>>,
    /// Whether the reply's steps were open.
    open: bool,
    cut: Vec<Piece>,
}

/// Keeps the transcript's list in step with the open chat: another chat starts over at its end,
/// items that arrive or go are spliced in, and the last item and the last piece of any reply still
/// streaming are measured again, since they can grow out of sight.
pub fn sync_transcript(w: &mut Workspace) {
    let Some(thread) = &w.thread else {
        return;
    };
    let same_chat = w.transcript_rows.0.as_deref() == Some(thread.id.as_str());
    let mut cut = std::mem::take(&mut w.transcript_pieces);
    if !same_chat {
        cut.clear();
    }
    let mut changed = false;
    let mut streaming = Vec::new();
    let mut shown = 0;
    for (i, m) in thread.messages.iter().filter(|m| !m.queued).enumerate() {
        shown = i + 1;
        let live = m.status == MessageStatus::Streaming;
        if live {
            streaming.push(i);
        }
        let open = m.role != MessageRole::User && is_open(w, m);
        let kept = cut
            .get(i)
            .is_some_and(|c| c.open == open && c.of.as_ref().is_some_and(|of| Arc::ptr_eq(of, m)));
        if kept {
            continue;
        }
        let pieces = if m.role == MessageRole::User {
            vec![Piece::Whole]
        } else {
            pieces_of(w, m)
        };
        changed |= cut.get(i).is_none_or(|c| c.id != m.id || c.cut != pieces);
        let next = Pieces {
            id: m.id.clone(),
            of: (!live).then(|| Arc::clone(m)),
            open,
            cut: pieces,
        };
        match cut.get_mut(i) {
            Some(c) => *c = next,
            None => cut.push(next),
        }
    }
    changed |= cut.len() != shown;
    cut.truncate(shown);
    let list = &w.transcript_list;
    if same_chat && !changed {
        let old = &w.transcript_rows.1;
        // Rows are in message order, so a message's last piece is found without a walk.
        let last_of = |message: usize| old.partition_point(|r| r.message <= message).checked_sub(1);
        for i in streaming
            .into_iter()
            .filter_map(last_of)
            .chain(old.len().checked_sub(1))
        {
            list.remeasure_items(i..i + 1);
        }
        w.transcript_pieces = cut;
        return;
    }
    let rows: Vec<Row> = cut
        .iter()
        .enumerate()
        .flat_map(|(message, c)| {
            c.cut.iter().map(move |&piece| Row {
                id: c.id.clone(),
                message,
                piece,
            })
        })
        .collect();
    if same_chat {
        let old = &w.transcript_rows.1;
        let same = old.iter().zip(&rows).take_while(|(a, b)| a == b).count();
        let tail = old[same..]
            .iter()
            .rev()
            .zip(rows[same..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        list.splice(same..old.len() - tail, rows.len() - same - tail);
    } else {
        list.reset(rows.len());
        list.set_follow_mode(FollowMode::Tail);
        w.rail.clear();
    }
    w.transcript_rows = (Some(thread.id.clone()), rows);
    w.transcript_pieces = cut;
}

/// How long the transcript's width holds before every row is measured again.
const WIDTH_SETTLE: Duration = Duration::from_millis(150);

/// While the transcript's width keeps changing, as when the window or the sidebar is dragged, it
/// is drawn from a list that lays out only the rows in reach. A new width throws away every row's
/// height, and the list that measures them all would lay out every row on every frame. Once the
/// width holds, every row is measured again, once.
///
/// A list just handed over has no bounds until it is laid out, so what reads them, as the turn
/// rail does, is drawn again on the frame after.
pub fn follow_width(w: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let width = w.transcript_list.viewport_bounds().size.width;
    // A list just handed over has not been laid out yet.
    if width == px(0.) {
        return;
    }
    let last = std::mem::replace(&mut w.transcript_width, width);
    if last == width || last == px(0.) {
        return;
    }
    if w.transcript_settle.is_none() {
        let in_reach = ListState::new(0, ListAlignment::Top, px(800.));
        w.transcript_list = handed_over(&w.transcript_list, in_reach);
        // This frame lays it out.
        cx.on_next_frame(window, |_, _, cx| cx.notify());
    }
    w.transcript_settle = Some(cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(WIDTH_SETTLE).await;
        let _ = this.update_in(cx, |w, window, cx| {
            w.transcript_settle = None;
            let all = ListState::new(0, ListAlignment::Top, px(800.)).measure_all();
            w.transcript_list = handed_over(&w.transcript_list, all);
            // The next frame lays it out.
            cx.on_next_frame(window, |_, window, cx| {
                cx.on_next_frame(window, |_, _, cx| cx.notify());
            });
            cx.notify();
        });
    }));
}

/// `to`, holding `from`'s rows and scrolled where `from` is.
fn handed_over(from: &ListState, to: ListState) -> ListState {
    to.reset(from.item_count());
    to.set_follow_mode(FollowMode::Tail);
    if !from.is_following_tail() {
        to.scroll_to(from.logical_scroll_top());
    }
    to
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
#[derive(Clone, Copy, PartialEq)]
enum Compaction {
    Compacting,
    Compacted,
    Stopped,
}

/// The state of a `/compact`, read from the reply after it.
fn compaction_state(reply: Option<&Message>, busy: bool) -> Compaction {
    // A /compact sent during a turn compacts where the turn is, and the turn's reply streams on
    // once it is done.
    let went_on = |r: &Message| {
        r.parts
            .iter()
            .position(|p| matches!(p, Part::Compaction { .. }))
            .is_some_and(|at| at + 1 < r.parts.len())
    };
    if reply.is_some_and(went_on) {
        return Compaction::Compacted;
    }
    match reply {
        Some(r) if r.status == MessageStatus::Streaming => Compaction::Compacting,
        None if busy => Compaction::Compacting,
        None => Compaction::Stopped,
        Some(r)
            if matches!(
                r.status,
                MessageStatus::Error | MessageStatus::Cancelled | MessageStatus::Interrupted
            ) =>
        {
            Compaction::Stopped
        }
        Some(_) => Compaction::Compacted,
    }
}

fn compacted_divider(
    row: SharedString,
    state: Compaction,
    before: Option<u64>,
    top: bool,
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
    div()
        .id(row)
        .role(Role::Status)
        .px(px(24.))
        .when(top, |d| d.pt(px(24.)))
        .child(
            div().flex().pb(px(20.)).child(
                div()
                    .flex()
                    .w_full()
                    .items_center()
                    .gap(px(12.))
                    .text_size(px(fs_xs()))
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
    let shown: Vec<&Message> = thread
        .messages
        .iter()
        .map(Arc::as_ref)
        .filter(|m| !m.queued)
        .collect();
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
            transcript_row(w, i, &before, marks.as_ref(), busy, &theme, cx)
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
                // The rail's strips reach 44px in from the edge when they widen.
                .when(rail::has_rail(w, &shown), |d| d.pr(px(28.)))
                .text_size(px(crate::ui::theme::fs_md()))
                .child(rows),
        )
        .child(crate::ui::selection::overlay(w.transcript_list.clone()))
        .children(list_scrollbar(&w.transcript_list, 10., 2., t.border_strong))
        .children(rail)
}

/// Row `i` of the open chat's transcript, drawn once the list scrolls it into reach.
fn transcript_row(
    w: &Workspace,
    i: usize,
    before: &[Option<u64>],
    marks: Option<&(String, Option<(String, usize)>)>,
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
    // The message, with the one before it and the one after.
    let mut around = thread
        .messages
        .iter()
        .map(Arc::as_ref)
        .filter(|m| !m.queued)
        .skip(message.saturating_sub(1));
    let previous = if message > 0 { around.next() } else { None };
    let Some(m) = around.next() else {
        return gpui::Empty.into_any_element();
    };
    let next = around.next();
    let first = i == 0 || rows[i - 1].message != message;
    let last = rows.get(i + 1).is_none_or(|next| next.message != message);
    let row = SharedString::from(match piece {
        Piece::Whole => format!("row-{}", m.id),
        Piece::Head => format!("row-{}-head", m.id),
        Piece::Step { step, from, .. } => format!("row-{}-{step}-{from}", m.id),
        Piece::Foot => format!("row-{}-foot", m.id),
    });
    // Whatever the chat's first message draws as keeps the gap above it.
    let top = first && message == 0;
    if m.role == MessageRole::User && rail::is_goal_clear(&m.text) {
        // The green notice in the reply already says the goal was cleared.
        return div()
            .id(row)
            .when(top, |d| d.pt(px(24.)))
            .into_any_element();
    }
    if m.role == MessageRole::User && upkeep(&m.text) == Some(Upkeep::Compact) {
        let was = before
            .get(message)
            .copied()
            .flatten()
            .filter(|_| w.settings.tokens.show_usage);
        return compacted_divider(row, compaction_state(next, busy), was, top, t)
            .into_any_element();
    }
    // The divider above a /compact already says how it went, so its reply shows nothing more
    // unless it said or did something besides compacting.
    let only_compacts = m.text.trim().is_empty()
        && m.parts.iter().all(|p| matches!(p, Part::Compaction { .. }))
        && matches!(m.status, MessageStatus::Streaming | MessageStatus::Complete);
    if m.role == MessageRole::Assistant
        && only_compacts
        && previous.is_some_and(|p| {
            p.role == MessageRole::User && upkeep(&p.text) == Some(Upkeep::Compact)
        })
    {
        return div().id(row).into_any_element();
    }
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
                .when(top, |d| d.pt(px(24.)))
                .when(m.role != MessageRole::User, |d| d.max_w(px(820.)))
                .child(body),
        )
        .into_any_element()
}
