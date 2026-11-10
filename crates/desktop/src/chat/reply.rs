//! A reply: its steps under a head line, then its files, notices and
//! actions.

// Rust guideline compliant 2026-10-09

use std::ops::Range;

use gpui::{AnyElement, ClipboardItem, Div, FontWeight, Role, SharedString, div, prelude::*, px};
use patchwork_server::threads::{Message, MessageStatus, Part, Role as MessageRole, StepStatus, Thread};

use crate::ui::theme::{FONT_MONO, FS_MD, FS_SM, FS_XS, RADIUS, RADIUS_SM, Theme};
use crate::ui::{button, format_clock, full_elapsed, icon, now_ms, spinner};
use crate::workspace::Workspace;

use crate::ui::markdown::{self, FindIn, markdown, markdown_part};
use super::timeline::{
    CommandResult, Entry, Step, action_of, command_result, details_of, entry_active, entry_label,
    fold_head_of, fold_layout_of, is_goal_cleared_reply, lines_of, shown, steps_of, work_key,
};
use super::transcript::Piece;
use super::{approvals, checks, files, interrupted, proposed, subagents};

/// `ACTION_CLASS`: a reply's small text buttons.
fn action_button(
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
const CALLS_PER_PIECE: usize = 10;

/// About how much of a long text a piece draws, in bytes.
const TEXT_PER_PIECE: usize = 600;

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
fn clears_goal(steps: &[Step]) -> bool {
    !steps.is_empty()
        && steps
            .iter()
            .all(|s| matches!(s, Step::Text(text) if is_goal_cleared_reply(text)))
}

/// Whether the reply's steps are open: opened by hand, or seen streaming and not closed since.
fn is_open(w: &Workspace, m: &Message) -> bool {
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
fn goal_cleared(
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

/// What sits under a reply's steps: approvals from before they had a place in the timeline, its
/// files and checks, a refusal or error, and its actions.
fn foot(
    w: &Workspace,
    thread: &Thread,
    m: &Message,
    previous: Option<&Message>,
    actions: bool,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Vec<AnyElement> {
    let mut out = Vec::new();
    // Replies saved before approvals had their place in the timeline show them here.
    let loose: Vec<&str> = m
        .approvals
        .iter()
        .filter(|a| {
            !m.parts
                .iter()
                .any(|p| matches!(p, Part::Approval { id } if id == &a.id))
        })
        .map(|a| a.id.as_str())
        .collect();
    if !loose.is_empty() {
        out.push(approvals::approvals(w, m, &loose, t, cx).into_any_element());
    }
    out.extend(files::reply_files(w, m, t, cx).map(IntoElement::into_any_element));
    out.extend(checks::checks_card(w, m, t, cx).map(IntoElement::into_any_element));
    if let Some(refusal) = &m.refusal {
        out.push(
            div()
                .id(SharedString::from(format!("refusal-{}", m.id)))
                .role(Role::Alert)
                .rounded(px(RADIUS))
                .bg(t.warning_soft)
                .px(px(12.))
                .py(px(8.))
                .text_size(px(FS_SM))
                .flex()
                .flex_col()
                .gap(px(4.))
                .border_1()
                .border_color(t.warning)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.warning)
                        .child(icon("shield-warning", 15.))
                        .child(match &refusal.category {
                            Some(category) => {
                                format!("Blocked by a safety classifier · {category}")
                            }
                            None => "Blocked by a safety classifier".into(),
                        }),
                )
                .child(div().text_color(t.text).child(refusal.explanation.clone()))
                .into_any_element(),
        );
    }
    if let Some(error) = &m.error {
        out.push(
            div()
                .id(SharedString::from(format!("error-{}", m.id)))
                .role(Role::Alert)
                .rounded(px(RADIUS))
                .bg(t.danger_soft)
                .px(px(12.))
                .py(px(8.))
                .text_size(px(FS_SM))
                .text_color(t.danger)
                .child(error.clone())
                .into_any_element(),
        );
    }
    if let Some(row) = interrupted::resume_row(&w.server, thread, m, t, cx) {
        out.push(row.into_any_element());
    }
    if actions && !m.cut_off && !m.text.trim().is_empty() {
        let streaming = m.status == MessageStatus::Streaming;
        // The buttons find their message when clicked, so drawing does not copy its text.
        let retry = previous
            .filter(|p| p.role == MessageRole::User)
            .map(|p| p.id.clone());
        let (thread_id, message_id) = (thread.id.clone(), m.id.clone());
        let copy_id = m.id.clone();
        out.push(
            div()
                .flex()
                .items_center()
                .when(streaming, gpui::Styled::invisible)
                .child(
                    action_button(format!("copy-{}", m.id), "Copy", "copy", t).on_click(
                        cx.listener(move |w, _, _, cx| {
                            if let Some(m) = w.message(&copy_id) {
                                cx.write_to_clipboard(ClipboardItem::new_string(m.text.clone()));
                            }
                        }),
                    ),
                )
                .children(retry.map(|retry_id| {
                    action_button(format!("retry-{}", m.id), "Retry", "arrow-clockwise", t)
                        .on_click(cx.listener(move |w, _, _, cx| {
                            if let Some(p) = w.message(&retry_id) {
                                let (text, attachments) = (p.text.clone(), p.attachments.clone());
                                w.send(text, attachments, None, None, cx);
                            }
                        }))
                }))
                .child(
                    action_button(format!("fork-{}", m.id), "Fork", "git-fork", t).on_click(
                        cx.listener(move |w, _, _, cx| w.fork(&thread_id, &message_id, cx)),
                    ),
                )
                .child(
                    div()
                        .ml(px(6.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .text_size(px(FS_XS))
                        .text_color(t.text_muted)
                        .child(format_clock(m.created_at)),
                )
                .into_any_element(),
        );
    }
    out
}

/// The reply's head line: "Working for 12s" while it streams, else its fold.
fn head(w: &Workspace, m: &Message, t: &Theme, cx: &mut Context<Workspace>) -> Div {
    let streaming = m.status == MessageStatus::Streaming;
    let steps = steps_of(m);
    let layout = fold_layout_of(m, &steps, streaming);
    let open = is_open(w, m);
    let head_box = |row: AnyElement| {
        div()
            .mb(px(6.))
            .border_b_1()
            .border_color(t.border)
            .pt(px(4.))
            .pb(px(8.))
            .child(row)
    };
    if streaming {
        head_box(
            div()
                .id(SharedString::from(format!("status-{}", m.id)))
                .role(Role::Status)
                .h(px(24.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(FS_MD))
                .text_color(t.text_muted)
                .child(format!(
                    "Working for {}",
                    full_elapsed(now_ms().saturating_sub(m.created_at))
                ))
                .into_any_element(),
        )
    } else {
        let head = fold_head_of(m, layout.actions, open);
        let id = m.id.clone();
        let hover = t.text;
        head_box(
            button(
                SharedString::from(format!("fold-{}", m.id)),
                head.label.clone(),
            )
            .aria_expanded(open)
            .h(px(24.))
            .gap(px(6.))
            .text_size(px(FS_MD))
            .text_color(t.text_muted)
            .hover(move |s| s.text_color(hover))
            .child(head.label)
            .children(
                head.action
                    .map(|a| div().text_color(t.text_subtle).child(a)),
            )
            .when(layout.actions > 0, |b| {
                b.child(icon(if open { "caret-down" } else { "caret-right" }, 13.))
            })
            .on_click(cx.listener(move |w, _, _, cx| {
                if !w.open_folds.remove(&id) {
                    w.open_folds.insert(id.clone());
                }
                cx.notify();
            }))
            .into_any_element(),
        )
    }
}

/// Step `i` of a reply; of a group of calls only the calls in `part`, of a text only its bytes in
/// `part`.
fn reply_step(
    w: &Workspace,
    m: &Message,
    i: usize,
    part: Range<usize>,
    find: Option<&FindIn>,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Div {
    let steps = steps_of(m);
    let Some(step) = steps.get(i) else {
        return div();
    };
    match step {
        Step::Text(text) => {
            // The texts above, drawn or folded, and this one's parts above, hold the matches
            // before this part's.
            if let Some(f) = find {
                let above: usize = steps[..i]
                    .iter()
                    .map(|s| match s {
                        Step::Text(text) => markdown::find_in(text, &f.needle).len(),
                        _ => 0,
                    })
                    .sum();
                let before = text.get(..part.start).unwrap_or_default();
                f.seen
                    .set(above + markdown::find_in(before, &f.needle).len());
            }
            let key = match part.start {
                0 => format!("{}-{i}", m.id),
                from => format!("{}-{i}-{from}", m.id),
            };
            div()
                .when(part.start == 0, |d| d.pt(px(2.)))
                .when(part.end >= text.len(), |d| d.pb(px(8.)))
                .child(markdown_part(
                    text.get(part.clone()).unwrap_or_default(),
                    key,
                    part.start > 0,
                    find,
                    cx,
                ))
        }
        Step::Plan(text) => div().pb(px(8.)).child(proposed::proposed_plan(
            w,
            format!("{}-plan-{i}", m.id),
            text,
            t,
            cx,
        )),
        Step::Work(entries) => div()
            .flex()
            .flex_col()
            .when(part.end >= entries.len(), |d| d.pb(px(8.)))
            .children(
                entries
                    .get(part)
                    .unwrap_or_default()
                    .iter()
                    .map(|e| entry_row(w, m, e, t, cx)),
            ),
        Step::Agents(tools) => subagents::subagents(w, m, tools, t, cx),
        Step::Approvals(ids) => approvals::approvals(w, m, ids, t, cx),
        Step::Compaction => div().pb(px(20.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .text_size(px(FS_XS))
                .text_color(t.text_subtle)
                .child(div().h(px(1.)).flex_1().bg(t.border))
                .child("Context compacted")
                .child(div().h(px(1.)).flex_1().bg(t.border)),
        ),
    }
}

/// One call or block of reasoning: its icon, label, result and mark, and
/// its details below once opened.
fn entry_row(
    w: &Workspace,
    m: &Message,
    entry: &Entry,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Div {
    let key = work_key(m, entry);
    let open = w.open_steps.contains(&key);
    let label = entry_label(entry);
    // A reply that stopped leaves its last steps at rest.
    let active = m.status == MessageStatus::Streaming && entry_active(entry);
    let glyph = match entry {
        Entry::Thinking { .. } => "brain",
        Entry::Tool(tool) => action_of(tool).icon(),
    };
    let pill = |text: String, color: gpui::Hsla| {
        div()
            .flex_none()
            .rounded(px(RADIUS_SM))
            .bg(t.text_tint_8)
            .px(px(6.))
            .font_family(FONT_MONO)
            .text_size(px(FS_XS))
            .line_height(px(18.))
            .text_color(color)
            .child(text)
    };
    let result = match entry {
        Entry::Tool(tool) => {
            if let Some((added, removed)) = lines_of(tool) {
                Some(
                    div()
                        .flex()
                        .gap(px(4.))
                        .child(pill(format!("+{added}"), t.success))
                        .child(pill(format!("−{removed}"), t.danger))
                        .into_any_element(),
                )
            } else {
                command_result(tool).map(|r| match r {
                    CommandResult::Exit(code) => {
                        pill(format!("exit {code}"), t.danger).into_any_element()
                    }
                    CommandResult::Tests { passed, failed } => {
                        let (text, color) = if failed > 0 {
                            (format!("{failed} failed"), t.danger)
                        } else {
                            (format!("{passed} passed"), t.success)
                        };
                        pill(text, color).into_any_element()
                    }
                })
            }
        }
        Entry::Thinking { .. } => None,
    };
    let mark = match entry {
        _ if active => Some(spinner(15., t.accent_text).into_any_element()),
        Entry::Tool(tool) if tool.status == StepStatus::Error => {
            Some(icon("x-bold", 14.).text_color(t.danger).into_any_element())
        }
        Entry::Tool(tool) if tool.status == StepStatus::Done => Some(
            icon("check-bold", 14.)
                .text_color(t.success)
                .into_any_element(),
        ),
        _ => None,
    };
    let details = open.then(|| match entry {
        Entry::Thinking { text, .. } => div()
            .ml(px(28.))
            .mt(px(2.))
            .mb(px(4.))
            .text_size(px(FS_SM))
            .text_color(t.text_muted)
            .child(markdown(text, format!("{key}-thinking"), cx))
            .into_any_element(),
        Entry::Tool(tool) => div()
            .id(SharedString::from(format!("{key}-details")))
            .ml(px(28.))
            .mt(px(2.))
            .mb(px(4.))
            .max_h(px(256.))
            .overflow_y_scroll()
            .rounded(px(RADIUS_SM))
            .bg(t.bg_editor)
            .px(px(8.))
            .py(px(6.))
            .font_family(FONT_MONO)
            .text_size(px(FS_SM))
            .text_color(t.text_muted)
            .child(details_of(tool))
            .into_any_element(),
    });
    let (fg, group) = (t.text, SharedString::from(format!("step-{key}")));
    let toggle = key.clone();
    div()
        .flex()
        .flex_col()
        .child(
            button(SharedString::from(format!("step-{key}")), label.clone())
                .group(group.clone())
                .aria_expanded(open)
                .min_h(px(24.))
                .w_full()
                .gap(px(6.))
                .py(px(2.))
                .text_size(px(FS_MD))
                .text_color(t.text_muted)
                .hover(move |s| s.text_color(fg))
                .child(
                    div()
                        .w(px(24.))
                        .flex_none()
                        .flex()
                        .opacity(0.85)
                        .child(icon(glyph, 17.)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().min_w_0().truncate().child(label))
                        .child(div().flex_1())
                        .children(result)
                        .child(
                            div()
                                .w(px(16.))
                                .flex_none()
                                .flex()
                                .justify_center()
                                .children(mark),
                        ),
                )
                .child(
                    div()
                        .invisible()
                        .group_hover(group, gpui::Styled::visible)
                        .text_color(t.text_subtle)
                        .child(icon(if open { "caret-down" } else { "caret-right" }, 12.)),
                )
                .on_click(cx.listener(move |w, _, _, cx| {
                    if !w.open_steps.remove(&toggle) {
                        w.open_steps.insert(toggle.clone());
                    }
                    cx.notify();
                })),
        )
        .children(details)
}
