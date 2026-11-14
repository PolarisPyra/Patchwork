//! The subagents a turn started: one as a card, several under
//! a summary row that opens to list them. They are their own step rather than rows in a work
//! group, since each has a thread of its own behind it.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Div, FontWeight, Hsla, SharedString, div, prelude::*, px};
use patchwork_server::provider::AgentKind;
use patchwork_server::threads::{Message, StepStatus, ToolCall};

use crate::chat::lineage::{is_active, status_label, title_of};
use crate::chat::timeline::tool_label;
use crate::ui::theme::{FS_XS, radius, radius_sm, Theme};
use crate::ui::{agent_logo, button, full_elapsed, icon, now_ms};
use crate::workspace::Workspace;

/// The span the subagents ran in, or none while one is still going.
fn span_of(subagents: &[&ToolCall]) -> Option<u64> {
    let start = subagents.iter().filter_map(|t| t.started_at).min()?;
    let ends: Vec<u64> = subagents.iter().filter_map(|t| t.ended_at).collect();
    (ends.len() == subagents.len()).then(|| {
        ends.iter()
            .max()
            .copied()
            .unwrap_or(start)
            .saturating_sub(start)
    })
}

fn dot_of(status: StepStatus, t: &Theme) -> Hsla {
    match status {
        StepStatus::Pending => t.text_subtle,
        StepStatus::Running => t.accent,
        StepStatus::Done => t.success,
        StepStatus::Error => t.danger,
    }
}

/// A round logo ringed in the editor's colour, so overlapping avatars stay apart.
fn avatar(agent: AgentKind, dot: Option<Hsla>, t: &Theme) -> Div {
    div()
        .relative()
        .flex_none()
        .rounded_full()
        .bg(t.bg_editor)
        .p(px(2.))
        .child(
            div()
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(t.border)
                .bg(t.bg_panel)
                .child(agent_logo(agent, None, 13.)),
        )
        .children(dot.map(|dot| {
            div()
                .absolute()
                .right(px(0.))
                .bottom(px(0.))
                .size(px(12.))
                .rounded_full()
                .border_2()
                .border_color(t.bg_editor)
                .bg(dot)
        }))
}

/// One subagent: what it was asked to do, what it is doing or found, and for how long. It opens
/// its own thread when it has one.
fn card(
    tool: &ToolCall,
    calls: &[ToolCall],
    agent: AgentKind,
    now: u64,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let active = is_active(tool);
    let title = title_of(tool);
    // While it works, what it is doing; once done, the first line of what it reported.
    let busy = calls
        .iter()
        .rev()
        .find(|c| c.parent.as_deref() == Some(tool.id.as_str()) && is_active(c));
    let reported = tool.output.as_deref().and_then(|o| {
        o.lines()
            .map(|l| l.replace(['`', '*', '_', '#', '>'], "").trim().to_string())
            .find(|l| !l.is_empty())
    });
    let detail = if active {
        busy.map(tool_label)
    } else {
        reported
    };
    let elapsed = tool
        .started_at
        .map(|s| tool.ended_at.unwrap_or(now).saturating_sub(s));
    let label = status_label(tool.status);
    let body = |d: gpui::Stateful<Div>| {
        d.w_full()
            .min_w_0()
            .gap(px(10.))
            .rounded(px(radius_sm()))
            .px(px(8.))
            .py(px(6.))
            .child(avatar(agent, Some(dot_of(tool.status, t)), t))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .min_w_0()
                            .items_baseline()
                            .gap(px(8.))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(FS_XS))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(t.text)
                                    .child(title.clone()),
                            )
                            .when(
                                detail.is_some() && (active || tool.status != StepStatus::Done),
                                |d| {
                                    d.child(
                                        div()
                                            .flex_none()
                                            .text_size(px(10.))
                                            .text_color(if tool.status == StepStatus::Error {
                                                t.danger
                                            } else {
                                                t.text_subtle
                                            })
                                            .child(label),
                                    )
                                },
                            ),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(t.text_muted)
                            .child(detail.clone().unwrap_or_else(|| label.to_string())),
                    ),
            )
            .children(elapsed.map(|e| {
                div()
                    .flex_none()
                    .text_size(px(FS_XS))
                    .text_color(t.text_subtle)
                    .child(full_elapsed(e))
            }))
    };
    match tool.child_thread_id.clone() {
        Some(child) => {
            let hover = t.bg_hover;
            body(button(
                SharedString::from(format!("subagent-card-{}", tool.id)),
                format!("Open {title}"),
            ))
            .hover(move |s| s.bg(hover))
            .child(
                icon("caret-right", 14.)
                    .flex_none()
                    .text_color(t.text_subtle),
            )
            .on_click(cx.listener(move |w, _, _, cx| w.open_thread(&child, cx)))
            .into_any_element()
        }
        None => body(
            div()
                .id(SharedString::from(format!("subagent-card-{}", tool.id)))
                .flex()
                .items_center(),
        )
        .into_any_element(),
    }
}

pub fn subagents(
    w: &Workspace,
    m: &Message,
    subagents: &[&ToolCall],
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Div {
    let agent = w.thread.as_ref().map_or(AgentKind::Claude, |t| t.agent);
    let now = now_ms();
    let out = div().pb(px(8.));
    if let [only] = subagents {
        return out.child(card(only, &m.tools, agent, now, t, cx));
    }
    let Some(first) = subagents.first() else {
        return out;
    };
    let key = first.id.clone();
    let open = w.open_agents.contains(&key);
    let working = subagents.iter().filter(|t| is_active(t)).count();
    let done = subagents
        .iter()
        .filter(|t| t.status == StepStatus::Done)
        .count();
    let failed = subagents
        .iter()
        .filter(|t| t.status == StepStatus::Error)
        .count();
    let summary = [(working, "working"), (done, "done"), (failed, "failed")]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect::<Vec<_>>()
        .join(" · ");
    let active = working > 0;
    let elapsed = if active {
        let start = subagents
            .iter()
            .map(|t| t.started_at.unwrap_or(now))
            .min()
            .unwrap_or(now);
        Some(now.saturating_sub(start))
    } else {
        span_of(subagents)
    };
    let count = subagents.len();
    let hover = t.bg_hover;
    let avatars = div()
        .flex()
        .flex_none()
        .items_center()
        .children(
            subagents
                .iter()
                .take(3)
                .enumerate()
                .map(|(i, _)| avatar(agent, None, t).when(i > 0, |d| d.ml(px(-6.)))),
        )
        .when(count > 3, |d| {
            d.child(
                div()
                    .ml(px(-6.))
                    .rounded_full()
                    .bg(t.bg_editor)
                    .p(px(2.))
                    .child(
                        div()
                            .h(px(24.))
                            .min_w(px(24.))
                            .px(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(t.bg_panel)
                            .text_size(px(10.))
                            .text_color(t.text_muted)
                            .child(format!("+{}", count - 3)),
                    ),
            )
        });
    let row = button(
        SharedString::from(format!("subagents-{key}")),
        format!("{count} subagents"),
    )
    .aria_expanded(open)
    .w_full()
    .min_w_0()
    .gap(px(10.))
    .rounded(px(radius_sm()))
    .px(px(8.))
    .py(px(6.))
    .hover(move |s| s.bg(hover))
    .text_color(if open || active { t.text } else { t.text_muted })
    .when(!(open || active), |d| d.opacity(0.55))
    .child(avatars)
    .child(
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .text_size(px(FS_XS))
            .child(
                div()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("{count} subagents")),
            )
            .child(
                div()
                    .truncate()
                    .text_color(if failed > 0 {
                        t.danger
                    } else if active {
                        t.accent_text
                    } else {
                        t.text_subtle
                    })
                    .child(summary),
            ),
    )
    .children(elapsed.map(|e| {
        div()
            .flex_none()
            .text_size(px(FS_XS))
            .text_color(t.text_subtle)
            .child(full_elapsed(e))
    }))
    .child(
        icon(if open { "caret-up" } else { "caret-down" }, 14.)
            .flex_none()
            .text_color(t.text_muted),
    )
    .on_click(cx.listener(move |w, _, _, cx| {
        if !w.open_agents.remove(&key) {
            w.open_agents.insert(key.clone());
        }
        cx.notify();
    }));
    out.child(row).when(open, |d| {
        d.child(
            div()
                .mt(px(4.))
                .flex()
                .flex_col()
                .gap(px(1.))
                .rounded(px(radius()))
                .border_1()
                .border_color(t.border)
                .bg(t.bg_panel)
                .p(px(4.))
                .children(
                    subagents
                        .iter()
                        .map(|tool| card(tool, &m.tools, agent, now, t, cx)),
                ),
        )
    })
}
