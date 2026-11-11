//! The steps of a reply: each tool call and what it did.

// Rust guideline compliant 2026-10-09

use std::ops::Range;
use gpui::{Div, SharedString, div, prelude::*, px};
use patchwork_server::threads::{Message, MessageStatus, StepStatus};
use crate::ui::theme::{FONT_MONO, FS_MD, FS_SM, FS_XS, RADIUS_SM, Theme};
use crate::ui::{button, icon, spinner};
use crate::workspace::Workspace;
use crate::ui::markdown::{self, FindIn, markdown, markdown_part};
use crate::chat::timeline::{
    CommandResult, Entry, Step, action_of, command_result, details_of, entry_active, entry_label,
    lines_of, steps_of, work_key,
};
use crate::chat::{approvals, proposed, subagents};

/// Step `i` of a reply; of a group of calls only the calls in `part`, of a text only its bytes in
/// `part`.
pub(super) fn reply_step(
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
pub(super) fn entry_row(
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
