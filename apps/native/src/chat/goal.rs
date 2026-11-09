//! The goal on the composer's top edge (`prompts/GoalBanner.tsx`, `hooks/useGoalClear.ts`): its
//! state and usage, then the objective. Its last state remains visible until the agent confirms
//! the goal is cleared.

use std::sync::Arc;
use gpui::{Context, FontWeight, Role, SharedString, Stateful, div, prelude::*, px};
use patchwork_server::provider::AgentKind;
use patchwork_server::threads::{Goal, GoalStatus};

use crate::theme::{FS_SM, FS_XS, RADIUS_SM, Theme};
use crate::ui::{button, format_tokens, full_elapsed, icon, now_ms, tooltip};
use crate::workspace::{Workspace, is_working};

/// A goal clear in the banner: pending while its error is none, failed otherwise. `ran` is set
/// once the clear's run is seen.
pub struct GoalClear {
    key: String,
    error: Option<String>,
    ran: bool,
}

fn label(goal: &Goal, working: bool) -> &'static str {
    match goal.status {
        GoalStatus::Active if !working => "Goal set",
        GoalStatus::Active => "Pursuing goal",
        GoalStatus::Paused => "Goal paused",
        GoalStatus::Blocked => "Goal blocked",
        GoalStatus::UsageLimited => "Goal stopped at usage limit",
        GoalStatus::BudgetLimited => "Goal reached token budget",
        GoalStatus::Complete => "Goal complete",
    }
}

/// A goal that stopped part way and can carry on. A blocked goal has been judged impossible.
fn can_resume(goal: &Goal) -> bool {
    matches!(
        goal.status,
        GoalStatus::Paused | GoalStatus::UsageLimited | GoalStatus::BudgetLimited
    )
}

/// The tokens, time, and checks of a goal, joined for the banner. Time shows from a minute.
fn usage(goal: &Goal, seconds: u64) -> Option<String> {
    let mut usage = Vec::new();
    if let Some(used) = goal.tokens_used.filter(|&n| n > 0) {
        usage.push(match goal.token_budget {
            None => format!("{} tokens", format_tokens(used)),
            Some(budget) => format!("{} / {} tokens", format_tokens(used), format_tokens(budget)),
        });
    }
    if seconds >= 60 {
        usage.push(full_elapsed(seconds * 1000));
    }
    match goal.checks {
        Some(1) => usage.push("1 check".into()),
        Some(n) if n > 0 => usage.push(format!("{n} checks")),
        _ => {}
    }
    (!usage.is_empty()).then(|| usage.join(" · "))
}

fn goal_key(thread_id: &str, goal: &Goal) -> String {
    format!("{thread_id}:{}", goal.started_at)
}

/// A small button on a strip above the composer.
pub fn strip_action(id: &'static str, label: &'static str, t: &Theme) -> Stateful<gpui::Div> {
    let hover = t.bg_hover;
    button(id, label)
        .flex_none()
        .h(px(24.))
        .px(px(10.))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_editor)
        .text_size(px(FS_XS))
        .text_color(t.text)
        .hover(move |s| s.bg(hover))
        .child(label)
}

impl Workspace {
    /// Clears the goal, in the agent's session too. The banner offers it once no turn is running.
    fn clear_goal(&mut self, cx: &mut Context<Self>) {
        let Some((id, goal)) = self
            .thread
            .as_ref()
            .and_then(|t| Some((t.id.clone(), t.goal.clone()?)))
        else {
            return;
        };
        let key = goal_key(&id, &goal);
        self.goal_clear = Some(GoalClear {
            key: key.clone(),
            error: None,
            ran: false,
        });
        let server = Arc::clone(&self.server);
        let task = cx.background_spawn(async move { server.stop_goal(&id) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                if let Err(e) = result
                    && let Some(clear) = this.goal_clear.as_mut().filter(|c| c.key == key)
                {
                    clear.error = Some(e);
                }
                this.reload(cx);
            })
        })
        .detach();
        cx.notify();
    }

    /// Codex takes /goal resume; Claude and Antigravity have no such command, so a message
    /// carries the goal on.
    fn resume_goal(&mut self, cx: &mut Context<Self>) {
        let Some(agent) = self.thread.as_ref().map(|t| t.agent) else {
            return;
        };
        let text = if agent == AgentKind::Codex {
            "/goal resume"
        } else {
            "Continue the goal."
        };
        self.send(text.into(), Vec::new(), None, None, cx);
    }
}

/// The open chat's goal banner, if it has a goal.
pub fn goal_banner(
    w: &mut Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<Stateful<gpui::Div>> {
    let thread = w.thread.as_ref()?;
    let goal = thread.goal.clone()?;
    let working = is_working(thread);
    let key = goal_key(&thread.id, &goal);
    // A clear still pending once its run has ended was not confirmed by the agent, so the
    // banner goes back to offering it.
    if let Some(clear) = w.goal_clear.as_mut().filter(|c| c.error.is_none()) {
        if working {
            clear.ran = true;
        } else if clear.ran {
            w.goal_clear = None;
        }
    }
    let clear = w.goal_clear.as_ref().filter(|c| c.key == key);
    let clearing = clear.is_some_and(|c| c.error.is_none());
    let clear_error = clear.and_then(|c| c.error.clone());
    let active = goal.status == GoalStatus::Active;
    // Only an active goal's elapsed time moves; the others show the time they stopped at.
    if active {
        w.tick_each_second(cx);
    }
    let seconds = goal
        .time_used_seconds
        .unwrap_or_else(|| now_ms().saturating_sub(goal.started_at) / 1000);
    let usage = usage(&goal, seconds);
    let label: SharedString = if clearing {
        "Clearing goal".into()
    } else {
        label(&goal, working).into()
    };
    let open = w.goal_closed != Some(goal.started_at);
    let (muted, fg) = (t.text_muted, t.text);
    let started = goal.started_at;
    let objective: SharedString = goal.objective.clone().into();
    let toggle = button("goal-toggle", label.clone())
        .aria_expanded(open)
        .flex_1()
        .min_w_0()
        .gap(px(8.))
        .text_color(muted)
        .hover(move |s| s.text_color(fg))
        .tooltip(tooltip(objective.clone()))
        .child(icon("target", 13.).flex_none().text_color(if active {
            t.accent_text
        } else {
            t.text_subtle
        }))
        .child(
            div()
                .id("goal-state")
                .role(Role::Status)
                .aria_label(label.clone())
                .flex_none()
                .whitespace_nowrap()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.text)
                .child(label),
        )
        .children(usage.map(|usage| {
            div()
                .min_w_0()
                .truncate()
                .text_size(px(FS_XS))
                .text_color(t.text_subtle)
                .child(usage)
        }))
        .child(
            icon(if open { "caret-down" } else { "caret-right" }, 12.)
                .ml_auto()
                .flex_none(),
        )
        .on_click(cx.listener(move |w, _, _, cx| {
            w.goal_closed = if w.goal_closed == Some(started) {
                None
            } else {
                Some(started)
            };
            cx.notify();
        }));
    let actions = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .when(!clearing && working && active, |d| {
            d.child(
                strip_action("goal-stop", "Stop", t)
                    .tooltip(tooltip("Pause the goal"))
                    .on_click(cx.listener(|w, _, _, cx| w.stop(cx))),
            )
        })
        .when(!clearing && !working && can_resume(&goal), |d| {
            d.child(
                strip_action("goal-resume", "Resume", t)
                    .tooltip(tooltip("Carry on with the goal"))
                    .on_click(cx.listener(|w, _, _, cx| w.resume_goal(cx))),
            )
        })
        .when(!clearing && !working && clear_error.is_some(), |d| {
            d.child(
                div()
                    .id("goal-clear-error")
                    .role(Role::Alert)
                    .aria_label("Couldn't clear goal")
                    .text_size(px(FS_XS))
                    .text_color(t.danger)
                    .child("Couldn't clear goal"),
            )
        })
        .when(!clearing && !working, |d| {
            let label = if clear_error.is_some() {
                "Retry"
            } else {
                "Clear goal"
            };
            d.child(
                strip_action("goal-clear", label, t)
                    .tooltip(tooltip("Drop the goal"))
                    .on_click(cx.listener(|w, _, _, cx| w.clear_goal(cx))),
            )
        });
    Some(
        div()
            .id("goal")
            .role(Role::Region)
            .aria_label("Goal")
            .mx(px(8.))
            .mb(px(6.))
            .overflow_hidden()
            .rounded(px(RADIUS_SM))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_panel)
            .text_size(px(FS_SM))
            .shadow_sm()
            .child(
                div()
                    .flex()
                    .h(px(36.))
                    .min_w_0()
                    .items_center()
                    .gap(px(8.))
                    .bg(t.bg_app)
                    .px(px(10.))
                    .child(toggle)
                    .child(actions),
            )
            .when(open, |d| {
                d.child(
                    div()
                        .id("goal-objective")
                        .tooltip(tooltip(objective.clone()))
                        .min_w_0()
                        .truncate()
                        .border_t_1()
                        .border_color(t.border)
                        .px(px(10.))
                        .py(px(6.))
                        .text_color(t.text)
                        .child(objective),
                )
            }),
    )
}
