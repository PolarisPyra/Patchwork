//! A reply cut into the pieces the chat lists, and the piece that says a goal was cleared.

// Rust guideline compliant 2026-10-09

use super::frame::foot;
use super::steps::reply_step;
use super::frame::head;
use gpui::{Div, SharedString, div, prelude::*, px};
use patchwork_server::threads::{Message, MessageStatus, Thread};
use crate::ui::theme::{FS_MD, FS_XS, RADIUS_SM, Theme};
use crate::ui::{button, icon};
use crate::workspace::Workspace;
use crate::ui::markdown::{self, FindIn};
use crate::chat::timeline::{Step, fold_layout_of, is_goal_cleared_reply, shown, steps_of};
use crate::chat::transcript::Piece;

/// `ACTION_CLASS`: a reply's small text buttons.
pub(super) fn action_button(
    id: String,
    label: &'static str,
    name: &'static str,
    t: &Theme,
) -> gpui::Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(SharedString::from(id), label)
        .h(px(24.))
        .gap(px(4.))
        .rounded(px(RADIUS_SM))
        .px(px(6.))
        .text_size(px(FS_XS))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon(name, 13.))
        .child(label)
}

/// Most calls of a group one piece holds, so a long run of calls is laid out a few at a time as
/// it scrolls into view.
pub(super) const CALLS_PER_PIECE: usize = 10;

/// About how much of a long text a piece draws, in bytes.
pub(super) const TEXT_PER_PIECE: usize = 600;

/// The pieces a reply is drawn in, one item of the transcript's list each, in the order they
/// show: its head line, each step it shows (a long group of calls a few calls at a time, a long
/// text a few blocks at a time), then its files, notices and actions.
pub fn pieces_of(w: &Workspace, m: &Message) -> Vec<Piece> {
    let steps = steps_of(m);
    if clears_goal(&steps) {
        return vec![Piece::Whole];
    }
    let streaming = m.status == MessageStatus::Streaming;
    let layout = fold_layout_of(m, &steps, streaming);
    let open = is_open(w, m);
    let mut pieces = Vec::new();
    if streaming || layout.folds {
        pieces.push(Piece::Head);
    }
    for (i, step) in steps.iter().enumerate() {
        if layout.folds && !open && !shown(m, step, i, layout.hidden) {
            continue;
        }
        match step {
            Step::Text(text) => pieces.extend(
                markdown::parts_of(text, TEXT_PER_PIECE)
                    .into_iter()
                    .map(|part| Piece::Step {
                        step: i,
                        from: part.start,
                        to: part.end,
                    }),
            ),
            Step::Work(entries) => pieces.extend((0..entries.len()).step_by(CALLS_PER_PIECE).map(
                |from| Piece::Step {
                    step: i,
                    from,
                    to: (from + CALLS_PER_PIECE).min(entries.len()),
                },
            )),
            _ => pieces.push(Piece::Step {
                step: i,
                from: 0,
                to: 0,
            }),
        }
    }
    pieces.push(Piece::Foot);
    pieces
}

/// A reply that only says the goal was cleared, which shows as one line.
pub(super) fn clears_goal(steps: &[Step]) -> bool {
    !steps.is_empty()
        && steps
            .iter()
            .all(|s| matches!(s, Step::Text(text) if is_goal_cleared_reply(text)))
}

/// Whether the reply's steps are open: opened by hand, or seen streaming and not closed since.
pub fn is_open(w: &Workspace, m: &Message) -> bool {
    w.open_folds.contains(&m.id) != w.streamed.contains(&m.id)
}

/// One piece of a reply.
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is a separate piece of what it draws"
)]
pub fn reply_piece(
    w: &Workspace,
    thread: &Thread,
    m: &Message,
    previous: Option<&Message>,
    piece: Piece,
    find: Option<&FindIn>,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Div {
    match piece {
        Piece::Whole => goal_cleared(w, thread, m, previous, t, cx),
        Piece::Head => head(w, m, t, cx),
        Piece::Step { step, from, to } => reply_step(w, m, step, from..to, find, t, cx),
        Piece::Foot => {
            let foot = foot(w, thread, m, previous, true, t, cx);
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .when(!foot.is_empty(), |d| d.pt(px(10.)))
                .children(foot)
        }
    }
}

/// A reply that only clears the goal: its line, then its files and notices.
pub(super) fn goal_cleared(
    w: &Workspace,
    thread: &Thread,
    m: &Message,
    previous: Option<&Message>,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Div {
    let text = steps_of(m)
        .iter()
        .filter_map(|s| match s {
            Step::Text(text) => Some(*text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(
            div()
                .min_h(px(24.))
                .flex()
                .gap(px(6.))
                .text_size(px(FS_MD))
                .text_color(t.text_muted)
                .child(
                    div()
                        .pt(px(3.))
                        .child(icon("check", 14.).text_color(t.text_subtle)),
                )
                .child(div().flex_1().min_w_0().child(text)),
        )
        .children(foot(w, thread, m, previous, false, t, cx))
}
