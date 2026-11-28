//! The goal on the composer's top edge: its state and usage, then the objective.

// Rust guideline compliant 2026-11-27

use crate::prelude::*;
use patchwork_platform::now_ms;
use std::sync::Arc;
use gpui::{FontWeight, Role, SharedString, Stateful, div, prelude::*, px};
use patchwork_client::local::core::chats::{Goal, GoalStatus};

use patchwork_design_icons::IconName;
use patchwork_design_theme::{fs_sm, fs_xs, radius_sm, Theme};
use patchwork_design_components::button;
use patchwork_design_text::full_elapsed;
use patchwork_design_icons::icon;
use crate::window::{is_working};

/// A goal clear that failed, for the goal `key` names.
pub struct GoalClear {
    key: String,
}

/// The tallest the objective grows before it scrolls: about five lines, so a long goal leaves the
/// chat above it in view.
const OBJECTIVE_MAX_HEIGHT: f32 = 100.;

fn label(goal: &Goal, working: bool) -> &'static str {
    match goal.status {
        GoalStatus::Active if !working => "Goal set",
        GoalStatus::Active => "Pursuing goal",
        GoalStatus::Paused => "Goal paused",
        GoalStatus::Stalled => "Goal stalled",
        GoalStatus::Blocked => "Goal blocked",
        GoalStatus::Complete => "Goal complete",
    }
}

/// A goal that stopped part way and can carry on. A blocked goal has been judged impossible.
fn can_resume(goal: &Goal) -> bool {
    matches!(goal.status, GoalStatus::Paused | GoalStatus::Stalled)
}

/// The time and checks of a goal, joined for the banner. Time shows from a minute.
fn usage(goal: &Goal, seconds: u64) -> Option<String> {
    let mut usage = Vec::new();
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

fn goal_key(chat_id: &str, goal: &Goal) -> String {
    format!("{chat_id}:{}", goal.started_at)
}

/// A small button on a strip above the composer.
#[must_use]
pub fn strip_action(id: &'static str, label: &'static str, t: &Theme) -> Stateful<gpui::Div> {
    let hover = t.bg_hover;
    button(id, label)
        .flex_none()
        .h(px(24.))
        .px(px(10.))
        .rounded(px(radius_sm()))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_editor)
        .text_size(px(fs_xs()))
        .text_color(t.text)
        .hover(move |s| s.bg(hover))
        .child(label)
}

/// The window's goal bar methods, for any [`ChatWindow`].
pub trait GoalBarExt: ChatWindow {
    /// Clears the goal. A reply still running finishes, and nothing carries the goal on after it.
    fn clear_goal(&mut self, cx: &mut Context<Self>) {
        let Some((id, goal)) = self
            .chat
            .as_ref()
            .and_then(|t| Some((t.id.clone(), t.goal.clone()?)))
        else {
            return;
        };
        let key = goal_key(&id, &goal);
        self.goal_clear = None;
        let core = Arc::clone(&self.core);
        patchwork_design_tasks::off_thread(
            cx,
            move || core.stop_goal(&id),
            move |this, result, cx| {
                if let Err(e) = result {
                    log::warn!(error:% = e; "failed to clear a goal: {{error}}");
                    this.goal_clear = Some(GoalClear { key });
                }
                this.reload(cx);
            },
        );
        cx.notify();
    }

    /// Carries the goal on in a new turn.
    fn resume_goal(&mut self, cx: &mut Context<Self>) {
        self.send("/goal resume".into(), Vec::new(), None, None, cx);
    }
}

impl<W: ChatWindow> GoalBarExt for W {}

/// The open chat's goal banner, if it has a goal.
pub fn goal_banner<W: ChatWindow>(
    w: &mut W,
    t: &Theme,
    cx: &mut Context<W>,
) -> Option<Stateful<gpui::Div>> {
    let chat = w.chat.as_ref()?;
    let goal = chat.goal.clone()?;
    let working = is_working(chat);
    let key = goal_key(&chat.id, &goal);
    let clear_failed = w.goal_clear.as_ref().is_some_and(|c| c.key == key);
    let active = goal.status == GoalStatus::Active;
    // Only an active goal's elapsed time moves; the others show the time they stopped at.
    if active {
        w.tick_each_second(cx);
    }
    let seconds = goal.seconds_used(now_ms());
    let usage = usage(&goal, seconds);
    let label: SharedString = label(&goal, working).into();
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
        .child(
            icon(IconName::Target, 13.)
                .flex_none()
                .text_color(if active { t.accent_text } else { t.text_subtle }),
        )
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
                .text_size(px(fs_xs()))
                .text_color(t.text_subtle)
                .child(usage)
        }))
        .child(
            icon(
                if open {
                    IconName::CaretDown
                } else {
                    IconName::CaretRight
                },
                12.,
            )
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
        .when(working && active, |d| {
            d.child(
                strip_action("goal-stop", "Stop", t)
                    .on_click(cx.listener(|w, _, _, cx| w.stop(cx))),
            )
        })
        .when(!working && can_resume(&goal), |d| {
            d.child(
                strip_action("goal-resume", "Resume", t)
                    .on_click(cx.listener(|w, _, _, cx| w.resume_goal(cx))),
            )
        })
        .when(clear_failed, |d| {
            d.child(
                div()
                    .id("goal-clear-error")
                    .role(Role::Alert)
                    .aria_label("Couldn't clear goal")
                    .text_size(px(fs_xs()))
                    .text_color(t.danger)
                    .child("Couldn't clear goal"),
            )
        })
        .child({
            let label = if clear_failed { "Retry" } else { "Clear goal" };
            strip_action("goal-clear", label, t)
                .on_click(cx.listener(|w, _, _, cx| w.clear_goal(cx)))
        });
    Some(
        div()
            .id("goal")
            .role(Role::Region)
            .aria_label("Goal")
            .mb(px(6.))
            .overflow_hidden()
            .rounded(px(radius_sm()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_panel)
            .text_size(px(fs_sm()))
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
                        .max_h(px(OBJECTIVE_MAX_HEIGHT))
                        .overflow_y_scroll()
                        .min_w_0()
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
