//! A thread's plan above the composer: the steps of the reply running now.

// Rust guideline compliant 2026-10-09

use std::time::Duration;

use gpui::{Div, FontWeight, Role, div, prelude::*, px, relative};
use patchwork_core::threads::{MessageStatus, PlanStep, Role as MessageRole, StepStatus};

use crate::ui::theme::{fs_sm, radius, Theme};
use crate::ui::{button, icon, mix, spinner};
use crate::workspace::{Workspace, is_working};

/// How long a finished list stays before it folds away.
const FOLD_MS: u64 = 1200;

/// The reply whose plan steps the strip shows, and the steps it held when they folded away.
pub struct ShownPlan {
    id: String,
    folded: Option<String>,
}

fn steps_key(steps: &[PlanStep]) -> String {
    steps
        .iter()
        .map(|s| format!("{:?} {}", s.status, s.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A plan's title: its first heading, else "Proposed plan".
pub fn plan_title(markdown: &str) -> String {
    markdown
        .lines()
        .find_map(|line| {
            let indent = line.len() - line.trim_start_matches(' ').len();
            let rest = line.trim_start_matches(' ');
            let hashes = rest.len() - rest.trim_start_matches('#').len();
            let title = rest[hashes..].strip_prefix([' ', '\t'])?.trim();
            (indent <= 3 && (1..=6).contains(&hashes) && !title.is_empty())
                .then(|| title.to_string())
        })
        .unwrap_or_else(|| "Proposed plan".to_string())
}

impl Workspace {
    /// The running reply's steps, kept while they fold away, and whether the strip shows them.
    /// A finished list, or one whose reply ended, stays a moment, then folds away; a new list
    /// brings it back. Lists that ended before the chat was opened are not shown.
    pub fn plan_steps(&mut self, cx: &mut Context<Self>) -> Vec<PlanStep> {
        let Some(thread) = &self.thread else {
            return Vec::new();
        };
        // Queued messages wait after the reply, so they are skipped.
        let Some(last) = thread.messages.iter().rev().find(|m| !m.queued) else {
            return Vec::new();
        };
        let live = is_working(thread)
            && last.role == MessageRole::Assistant
            && last.status == MessageStatus::Streaming
            && !last.plan.is_empty();
        if live && self.shown_plan.as_ref().is_none_or(|s| s.id != last.id) {
            self.shown_plan = Some(ShownPlan {
                id: last.id.clone(),
                folded: None,
            });
        }
        let shown = self.shown_plan.as_ref();
        if last.role != MessageRole::Assistant
            || last.plan.is_empty()
            || !(live || shown.is_some_and(|s| s.id == last.id))
        {
            return Vec::new();
        }
        let steps = last.plan.clone();
        let key = steps_key(&steps);
        let over = !live || steps.iter().all(|s| s.status == StepStatus::Done);
        let folded = over && shown.is_some_and(|s| s.folded.as_deref() == Some(key.as_str()));
        if folded {
            return Vec::new();
        }
        if over && self.plan_fold_queued.as_deref() != Some(key.as_str()) {
            self.plan_fold_queued = Some(key.clone());
            let id = last.id.clone();
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(FOLD_MS))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.plan_fold_queued.as_deref() == Some(key.as_str())
                        && let Some(shown) = this.shown_plan.as_mut().filter(|s| s.id == id)
                    {
                        shown.folded = Some(key);
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        steps
    }
}

fn step_icon(status: StepStatus, t: &Theme) -> gpui::AnyElement {
    let slot = div().flex_none().mt(px(2.));
    match status {
        StepStatus::Done => slot
            .child(icon("check", 14.).text_color(t.accent_text))
            .into_any_element(),
        StepStatus::Running => slot.child(spinner(14., t.accent_text)).into_any_element(),
        StepStatus::Error => slot
            .child(icon("x-circle", 14.).text_color(t.danger))
            .into_any_element(),
        StepStatus::Pending => slot
            .child(icon("circle", 14.).text_color(t.text_subtle))
            .into_any_element(),
    }
}

/// The agent's plan above the composer: how far it got, opening on its steps. It arrives as one
/// line: arriving open took height from the chat at every step and moved it up.
pub fn plan_panel(
    w: &mut Workspace,
    steps: &[PlanStep],
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Div {
    let open = w.plan_open;
    let done = steps
        .iter()
        .filter(|s| s.status == StepStatus::Done)
        .count();
    // An agent can leave a step running while it moves on to a later one; the latest running step
    // is the one it is on. A stopped turn puts its running step back to pending, and the header
    // keeps naming it.
    let running = steps.iter().rev().find(|s| s.status == StepStatus::Running);
    if let Some(running) = running {
        w.plan_last_running = Some(running.text.clone());
    }
    let current = running
        .or_else(|| {
            steps.iter().find(|s| {
                Some(&s.text) == w.plan_last_running.as_ref() && s.status == StepStatus::Pending
            })
        })
        .or_else(|| steps.iter().find(|s| s.status == StepStatus::Pending));
    let fg = t.text;
    let fraction = if steps.is_empty() {
        0.
    } else {
        done as f32 / steps.len() as f32
    };
    div()
        .mb(px(6.))
        .rounded(px(radius()))
        .border_1()
        .border_color(t.border)
        .bg(t.bg_panel)
        .text_size(px(fs_sm()))
        .child(
            button("plan-steps", "Tasks")
                .aria_expanded(open)
                .w_full()
                .min_w_0()
                .gap(px(6.))
                .px(px(10.))
                .py(px(6.))
                .text_color(t.text_muted)
                .hover(move |s| s.text_color(fg))
                .child(icon("list-checks", 14.).flex_none())
                .child(
                    div()
                        .flex_none()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.text)
                        .child(format!("Tasks {done}/{}", steps.len())),
                )
                .children(
                    current.map(|s| div().flex_1().min_w_0().truncate().child(s.text.clone())),
                )
                .when(!steps.is_empty(), |b| {
                    b.child(
                        div()
                            .ml_auto()
                            .flex_none()
                            .h(px(6.))
                            .w(px(64.))
                            .overflow_hidden()
                            .rounded_full()
                            .bg(mix(t.text_subtle, t.bg_panel, 25.))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(fraction))
                                    .rounded_full()
                                    .bg(t.accent_text),
                            ),
                    )
                })
                .child(icon(if open { "caret-down" } else { "caret-right" }, 12.).flex_none())
                .on_click(cx.listener(|w, _, _, cx| {
                    w.plan_open = !w.plan_open;
                    cx.notify();
                })),
        )
        .when(open, |d| {
            d.child(
                div()
                    .id("plan-step-list")
                    .role(Role::List)
                    .aria_label("Tasks")
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .border_t_1()
                    .border_color(t.border)
                    .px(px(12.))
                    .py(px(10.))
                    .children(steps.iter().map(|step| {
                        div()
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .child(step_icon(step.status, t))
                            .child(step.text.clone())
                    })),
            )
        })
}
