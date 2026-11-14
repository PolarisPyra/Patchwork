//! Subagents of the current turn, over the transcript's top right: running ones, then previous ones, with a
//! menu to disconnect the agent session or close the card.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::time::Duration;

use gpui::{
    Div, FocusHandle, deferred, FontWeight, Role, SharedString, Stateful, Window, div, prelude::*,
    px, relative,
};
use patchwork_server::provider::AgentKind;
use patchwork_server::threads::{Role as MessageRole, StepStatus, Thread, ToolCall};

use crate::ui::theme::{font_mono, fs_sm, fs_xs, radius, radius_lg, Theme};
use crate::ui::{
    agent_logo, button, compact_elapsed, full_elapsed, icon, now_ms, popup, popup_shadow, spinner,
};
use crate::workspace::{Workspace, is_working};

/// How long a row is hovered before its details card opens.
const HOVER_DELAY_MS: u64 = 200;

/// The card's state: the newest subagent per thread when it was closed, as it reopens once
/// another starts, and what is open in it.
#[derive(Default)]
pub struct Lineage {
    closed_at: HashMap<String, String>,
    previous_open: bool,
    menu_open: bool,
    /// The row whose details show, and the one waiting out the hover delay.
    details: Option<String>,
    hovering: Option<String>,
    focus: HashMap<String, FocusHandle>,
}

/// Tool calls of the current turn: every assistant message since the last non-steer user message.
fn current_turn_tools(thread: &Thread) -> Vec<&ToolCall> {
    let start = thread
        .messages
        .iter()
        .rposition(|m| m.role == MessageRole::User && !m.steer)
        .map_or(0, |i| i + 1);
    thread.messages[start..]
        .iter()
        .filter(|m| m.role == MessageRole::Assistant)
        .flat_map(|m| &m.tools)
        .collect()
}

/// Running or about to run. Finished calls are done or failed.
pub(super) fn is_active(tool: &ToolCall) -> bool {
    matches!(tool.status, StepStatus::Running | StepStatus::Pending)
}

/// What a call is called in a row: a subagent's own description, else its kind, else the tool's.
pub(super) fn title_of(tool: &ToolCall) -> String {
    tool.subagent
        .as_ref()
        .and_then(|s| {
            [&s.description, &s.kind]
                .into_iter()
                .find(|v| !v.is_empty())
                .cloned()
        })
        .unwrap_or_else(|| tool.name.clone())
}

/// Time a call has run, or none when its start is unknown.
fn elapsed_ms(tool: &ToolCall, now: u64) -> Option<u64> {
    let started = tool.started_at?;
    Some(tool.ended_at.unwrap_or(now).saturating_sub(started))
}

pub(super) fn status_label(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "Queued",
        StepStatus::Running => "Running",
        StepStatus::Done => "Completed",
        StepStatus::Error => "Failed",
    }
}

impl Workspace {
    fn hover_subagent(&mut self, id: String, hovered: bool, cx: &mut Context<Self>) {
        if !hovered {
            if self.lineage.hovering.as_ref() == Some(&id) {
                self.lineage.hovering = None;
            }
            if self.lineage.details.as_ref() == Some(&id) {
                self.lineage.details = None;
                cx.notify();
            }
            return;
        }
        self.lineage.hovering = Some(id.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(HOVER_DELAY_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.lineage.hovering.as_ref() == Some(&id) {
                    this.lineage.details = Some(id);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn stop_subagent(&mut self, tool_id: &str, cx: &mut Context<Self>) {
        let Some(thread) = self.thread.as_ref().map(|t| t.id.clone()) else {
            return;
        };
        if let Err(e) = self.server.stop_subagent(&thread, tool_id) {
            self.error = Some(e.to_string());
        }
        cx.notify();
    }
}

/// Floating card to the left of a row: what the subagent is and what it did last.
fn details_card(tool: &ToolCall, tools: &[&ToolCall], elapsed: Option<u64>, t: &Theme) -> Div {
    let last = tools
        .iter()
        .rfind(|c| c.parent.as_deref() == Some(tool.id.as_str()) && c.subagent.is_none());
    let model = tool
        .subagent
        .as_ref()
        .and_then(|s| s.model.clone())
        .unwrap_or_else(|| "inherits model".into());
    let kind = tool
        .subagent
        .as_ref()
        .map(|s| s.kind.clone())
        .unwrap_or_default();
    let (status_icon, color) = match tool.status {
        StepStatus::Running => (Some(spinner(12., t.accent).into_any_element()), t.accent),
        StepStatus::Done => (
            Some(icon("check", 12.).text_color(t.success).into_any_element()),
            t.success,
        ),
        StepStatus::Error => (
            Some(
                icon("x-circle", 12.)
                    .text_color(t.danger)
                    .into_any_element(),
            ),
            t.danger,
        ),
        StepStatus::Pending => (None, t.text_subtle),
    };
    div()
        .absolute()
        .top_0()
        .right(relative(1.))
        .mr(px(8.))
        .w(px(256.))
        .flex()
        .flex_col()
        .gap(px(6.))
        .rounded(px(radius()))
        .border_1()
        .border_color(t.border)
        .bg(t.bg_panel)
        .p(px(10.))
        .text_size(px(fs_sm()))
        .shadow(popup_shadow(t))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.text)
                .child(title_of(tool)),
        )
        .child(
            div()
                .text_color(t.text_muted)
                .child(format!("{model} · {kind}")),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .children(status_icon)
                .child(div().text_color(color).child(status_label(tool.status)))
                .children(elapsed.map(|ms| {
                    div()
                        .ml_auto()
                        .text_color(t.text_muted)
                        .child(full_elapsed(ms))
                })),
        )
        .children(last.map(|last| {
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(px(6.))
                .font_family(font_mono())
                .text_size(px(fs_xs()))
                .text_color(t.text_muted)
                .child(icon("terminal", 12.).flex_none())
                .child(
                    div()
                        .truncate()
                        .child(format!("{} {}", last.name, last.summary)),
                )
        }))
}

/// One subagent: provider avatar with a status dot, title, elapsed time, and a stop button while
/// running.
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is a separate piece of what it draws"
)]
fn subagent_row(
    lineage: &mut Lineage,
    thread: &Thread,
    tool: &ToolCall,
    tools: &[&ToolCall],
    now: u64,
    t: &Theme,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let focus = lineage
        .focus
        .entry(tool.id.clone())
        .or_insert_with(|| cx.focus_handle())
        .clone();
    let elapsed = elapsed_ms(tool, now);
    let stoppable = tool.status == StepStatus::Running && thread.agent == AgentKind::Claude;
    let title = title_of(tool);
    let dot = match tool.status {
        StepStatus::Pending => t.text_subtle,
        StepStatus::Running => t.accent,
        StepStatus::Done => t.success,
        StepStatus::Error => t.danger,
    };
    let hover = t.bg_hover;
    let group = SharedString::from(format!("subagent-{}", tool.id));
    let trailing = if tool.status == StepStatus::Error {
        div()
            .text_size(px(fs_xs()))
            .text_color(t.danger)
            .child(status_label(StepStatus::Error))
            .into_any_element()
    } else {
        div()
            .when(stoppable, |d| {
                d.group_hover(group.clone(), |s| s.opacity(0.))
            })
            .text_size(px(fs_xs()))
            .text_color(t.text_subtle)
            .children(elapsed.map(compact_elapsed))
            .into_any_element()
    };
    let stop = stoppable.then(|| {
        let id = tool.id.clone();
        button(
            SharedString::from(format!("stop-subagent-{}", tool.id)),
            format!("Stop subagent {title}"),
        )
        .tooltip(crate::ui::tooltip("Stop subagent"))
        .absolute()
        .right_0()
        .size(px(20.))
        .justify_center()
        .rounded(px(radius()))
        .text_color(t.danger)
        .opacity(0.)
        .group_hover(group.clone(), |s| s.opacity(1.))
        .focus(|s| s.opacity(1.))
        .hover(move |s| s.bg(hover))
        .child(icon("square-fill", 10.))
        .on_click(cx.listener(move |w, _, _, cx| {
            // The row behind opens the thread; the stop button only stops.
            cx.stop_propagation();
            w.stop_subagent(&id, cx);
        }))
    });
    // Focus shows the details too, so the keyboard reaches what hovering shows.
    let shown = lineage.details.as_deref() == Some(tool.id.as_str()) || focus.is_focused(window);
    let hover_id = tool.id.clone();
    div()
        .id(SharedString::from(format!("subagent-row-{}", tool.id)))
        .group(group)
        .role(Role::ListItem)
        .aria_label(title.clone())
        .tab_index(0)
        .relative()
        .flex()
        .min_w_0()
        .items_center()
        .gap(px(8.))
        .rounded(px(radius()))
        .px(px(6.))
        .py(px(4.))
        .hover(move |s| s.bg(hover))
        .on_hover(cx.listener(move |w, hovered: &bool, _, cx| {
            w.hover_subagent(hover_id.clone(), *hovered, cx);
        }))
        .track_focus(&focus)
        // Opens the subagent's own thread, as its card in the chat does.
        .when_some(tool.child_thread_id.clone(), |d, child| {
            d.role(Role::Button)
                .aria_label(format!("Open {title}"))
                .cursor_pointer()
                .on_click(cx.listener(move |w, _, _, cx| w.open_thread(&child, cx)))
        })
        .child(
            div()
                .relative()
                .flex_none()
                .size(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .child(agent_logo(thread.agent, thread.model.as_deref(), 16.))
                .child(
                    div()
                        .absolute()
                        .right_0()
                        .bottom_0()
                        .size(px(9.))
                        .rounded_full()
                        .border_2()
                        .border_color(t.bg_panel)
                        .bg(dot),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.text)
                .child(title),
        )
        .child(
            div()
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .justify_end()
                .min_w(px(20.))
                .child(trailing)
                .children(stop),
        )
        .when(shown, |d| d.child(details_card(tool, tools, elapsed, t)))
}

/// The subagents the card lists as running, while it is open. The background strip leaves these
/// to the card, so a running subagent is listed once.
pub(super) fn running_ids(w: &Workspace) -> Vec<String> {
    let Some(thread) = w.thread.as_ref() else {
        return Vec::new();
    };
    let subagents: Vec<&ToolCall> = current_turn_tools(thread)
        .into_iter()
        .filter(|c| c.subagent.is_some())
        .collect();
    let closed = subagents
        .last()
        .is_none_or(|newest| w.lineage.closed_at.get(&thread.id) == Some(&newest.id));
    if closed {
        return Vec::new();
    }
    subagents
        .into_iter()
        .filter(|c| is_active(c))
        .map(|c| c.id.clone())
        .collect()
}

/// The card, while the current turn has subagents and it was not closed since the newest started.
pub fn lineage_card(
    w: &mut Workspace,
    t: &Theme,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Option<Stateful<Div>> {
    let Workspace {
        thread, lineage, ..
    } = w;
    let thread = thread.as_ref()?;
    let tools = current_turn_tools(thread);
    let subagents: Vec<&ToolCall> = tools
        .iter()
        .copied()
        .filter(|c| c.subagent.is_some())
        .collect();
    let newest = subagents.last()?.id.clone();
    if lineage.closed_at.get(&thread.id) == Some(&newest) {
        return None;
    }
    let ticking = subagents.iter().any(|c| is_active(c));
    let now = now_ms();
    let working = is_working(thread);
    let (active, previous): (Vec<&ToolCall>, Vec<&ToolCall>) =
        subagents.iter().partition(|c| is_active(c));
    let running = subagents
        .iter()
        .filter(|c| c.status == StepStatus::Running)
        .count();
    let rows = |list: &[&ToolCall],
                label: &'static str,
                lineage: &mut Lineage,
                cx: &mut Context<Workspace>| {
        let rows: Vec<_> = list
            .iter()
            .map(|tool| subagent_row(lineage, thread, tool, &tools, now, t, window, cx))
            .collect();
        div()
            .id(label)
            .role(Role::List)
            .aria_label(label)
            .flex()
            .flex_col()
            .gap(px(1.))
            .px(px(6.))
            .pb(px(6.))
            .children(rows)
    };
    let (fg, subtle, hover) = (t.text, t.text_subtle, t.bg_hover);
    let menu_open = lineage.menu_open;
    let menu = menu_open.then(|| {
        let item = |id: &'static str, label: &'static str, name: &'static str, enabled: bool| {
            button(id, label)
                .role(Role::MenuItem)
                .w_full()
                .gap(px(8.))
                .rounded(px(radius()))
                .px(px(8.))
                .py(px(6.))
                .text_color(if enabled { fg } else { subtle })
                .when(enabled, |b| b.hover(move |s| s.bg(hover)))
                .when(!enabled, gpui::Styled::cursor_default)
                .child(icon(name, 14.).flex_none().text_color(t.text_muted))
                .child(label)
        };
        let thread_id = thread.id.clone();
        popup(t)
            .id("lineage-menu")
            .role(Role::Menu)
            .aria_label("Lineage actions")
            .absolute()
            .top_full()
            .right_0()
            .mt(px(4.))
            .w(px(224.))
            .occlude()
            .on_mouse_down_out(cx.listener(|w, _, _, cx| {
                w.lineage.menu_open = false;
                cx.notify();
            }))
            .child(
                item(
                    "lineage-disconnect",
                    "Disconnect agent session",
                    "plugs",
                    working,
                )
                .when(working, |b| {
                    b.on_click(cx.listener(|w, _, _, cx| {
                        w.lineage.menu_open = false;
                        w.stop(cx);
                    }))
                }),
            )
            .child(
                item("lineage-close", "Close lineage", "x", true).on_click(cx.listener(
                    move |w, _, _, cx| {
                        w.lineage.menu_open = false;
                        w.lineage
                            .closed_at
                            .insert(thread_id.clone(), newest.clone());
                        cx.notify();
                    },
                )),
            )
    });
    let active_rows = (!active.is_empty()).then(|| rows(&active, "Running agents", lineage, cx));
    let previous_open = lineage.previous_open;
    let previous_list = (previous_open && !previous.is_empty())
        .then(|| rows(&previous, "Previous agents", lineage, cx));
    let card = div()
        .id("lineage")
        .role(Role::Region)
        .aria_label("Lineage")
        .w(px(280.))
        .rounded(px(radius_lg()))
        .border_1()
        .border_color(t.border)
        .bg(t.bg_panel)
        .text_size(px(fs_sm()))
        .shadow(popup_shadow(t))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(8.))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(px(fs_xs()))
                        .text_color(t.text_muted)
                        .child(if running > 0 {
                            format!("Lineage · {running} running")
                        } else {
                            "Lineage".into()
                        }),
                )
                .child(
                    div()
                        .relative()
                        .child(
                            button("lineage-actions", "Lineage actions")
                                .aria_expanded(menu_open)
                                .size(px(24.))
                                .flex_none()
                                .justify_center()
                                .rounded(px(radius()))
                                .text_color(subtle)
                                .hover(move |s| s.bg(hover).text_color(fg))
                                .child(icon("dots-three", 14.))
                                .on_click(cx.listener(|w, _, _, cx| {
                                    w.lineage.menu_open = !w.lineage.menu_open;
                                    cx.notify();
                                })),
                        )
                        .children(menu.map(|menu| deferred(menu).with_priority(1))),
                ),
        )
        .children(active_rows)
        .when(!previous.is_empty(), |d| {
            d.child(
                div()
                    .border_t_1()
                    .border_color(t.border)
                    .child(
                        button("lineage-previous", "Previous agents")
                            .aria_expanded(previous_open)
                            .w_full()
                            .gap(px(6.))
                            .px(px(12.))
                            .py(px(6.))
                            .text_size(px(fs_xs()))
                            .text_color(t.text_muted)
                            .hover(move |s| s.text_color(fg))
                            .child(
                                icon(
                                    if previous_open {
                                        "caret-down"
                                    } else {
                                        "caret-right"
                                    },
                                    10.,
                                )
                                .flex_none(),
                            )
                            .child(if previous_open {
                                "Previous agents".to_string()
                            } else {
                                format!("Previous agents ({})", previous.len())
                            })
                            .on_click(cx.listener(|w, _, _, cx| {
                                w.lineage.previous_open = !w.lineage.previous_open;
                                cx.notify();
                            })),
                    )
                    .children(previous_list),
            )
        });
    // Ticks the elapsed times once a second, only while a subagent is still going.
    if ticking {
        w.tick_each_second(cx);
    }
    Some(card)
}
