//! Tasks the agent started that are still running after its reply ended.
//! They keep the turn going, so the user can follow them from here. Tasks that Patchwork closing
//! ended stay listed until the user runs them again or dismisses them.

// Rust guideline compliant 2026-10-09

use gpui::{Div, FontWeight, Role, SharedString, Stateful, div, prelude::*, px, relative};
use patchwork_core::threads::{BackgroundTask, TaskStatus};

use crate::ui::theme::{fs_sm, radius, Theme};
use crate::ui::{button, icon_button, now_ms, spinner, tooltip};
use crate::workspace::Workspace;

/// What each kind of task the agents report is called here. Any other kind is shown as it comes.
fn kind_label(kind: &str) -> &str {
    match kind {
        "command" | "local_bash" => "Command",
        "agent" | "local_agent" => "Agent",
        "local_workflow" => "Workflow",
        "mcp_task" => "MCP",
        "local_memory_import" => "Memory",
        other => other,
    }
}

/// Elapsed time as m:ss, or h:mm:ss from an hour on.
fn elapsed(ms: u64) -> String {
    let total = ms / 1000;
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

impl Workspace {
    /// Drops task `tool_id` from the tasks that Patchwork closing ended.
    fn dismiss_cut_off(&mut self, tool_id: String, cx: &mut Context<Self>) {
        let Some(thread) = self.thread.as_ref().map(|t| t.id.clone()) else {
            return;
        };
        self.change(
            "dismiss a stopped background task",
            move |core| core.dismiss_cut_off_task(&thread, &tool_id),
            cx,
        );
    }

    /// Asks the agent to start `task` again, which Patchwork closing ended.
    fn run_cut_off_again(&mut self, task: &BackgroundTask, cx: &mut Context<Self>) {
        let kind = kind_label(&task.kind).to_lowercase();
        self.send(
            format!(
                "Start this background {kind} again; it stopped when Patchwork closed: {}",
                task.description
            ),
            Vec::new(),
            None,
            None,
            cx,
        );
        self.dismiss_cut_off(task.tool_id.clone(), cx);
    }
}

/// A row of a task that Patchwork closing ended, with Run again and Dismiss.
fn cut_off_row(i: usize, task: BackgroundTask, t: &Theme, cx: &mut Context<Workspace>) -> Div {
    let hover = t.bg_hover;
    let tool_id = task.tool_id.clone();
    let description = task.description.clone();
    div()
        .flex()
        .min_w_0()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.text)
                .child(kind_label(&task.kind).to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(t.text_muted)
                .child(description.clone()),
        )
        .child(
            div()
                .flex_none()
                .text_color(t.text_muted)
                .child("Stopped when Patchwork closed"),
        )
        .child(
            button(
                SharedString::from(format!("run-again-{i}")),
                format!("Run {description} again"),
            )
            .flex_none()
            .h(px(24.))
            .px(px(10.))
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_editor)
            .text_color(t.text)
            .hover(move |s| s.bg(hover))
            .child("Run again")
            .on_click(cx.listener(move |w, _, _, cx| w.run_cut_off_again(&task, cx))),
        )
        .child(
            icon_button(
                SharedString::from(format!("dismiss-cut-off-{i}")),
                format!("Dismiss {description}"),
                "x",
                12.,
                cx,
            )
            .flex_none()
            .tooltip(tooltip("Dismiss"))
            .on_click(cx.listener(move |w, _, _, cx| w.dismiss_cut_off(tool_id.clone(), cx))),
        )
}

pub fn background_tasks(
    w: &mut Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<Stateful<Div>> {
    // A running subagent is listed in the lineage card, so not here as well.
    let in_lineage = super::lineage::running_ids(w);
    let tasks = w
        .thread
        .as_ref()?
        .messages
        .iter()
        .flat_map(|m| &m.background);
    let live: Vec<BackgroundTask> = tasks
        .clone()
        .filter(|task| task.status.is_live() && !in_lineage.contains(&task.tool_id))
        .cloned()
        .collect();
    let cut_off: Vec<BackgroundTask> = tasks.filter(|task| task.cut_off).cloned().collect();
    if live.is_empty() && cut_off.is_empty() {
        return None;
    }
    if !live.is_empty() {
        w.tick_each_second(cx);
    }
    let cut_off: Vec<Div> = cut_off
        .into_iter()
        .enumerate()
        .map(|(i, task)| cut_off_row(i, task, t, cx))
        .collect();
    let now = now_ms();
    let rows = live.into_iter().map(|task| {
        let lead = if task.status == TaskStatus::Running {
            div()
                .flex_none()
                .child(spinner(12., t.accent))
                .into_any_element()
        } else {
            div()
                .flex_none()
                .text_color(t.text_muted)
                .child("Waiting")
                .into_any_element()
        };
        div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(px(8.))
            .child(lead)
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.text)
                    .child(kind_label(&task.kind).to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(t.text_muted)
                    .child(task.description.clone()),
            )
            .children(task.summary.clone().map(|summary| {
                div()
                    .flex_none()
                    .min_w_0()
                    .max_w(relative(0.4))
                    .truncate()
                    .text_color(t.text_muted)
                    .child(summary)
            }))
            .child(
                div()
                    .flex_none()
                    .text_color(t.text_muted)
                    .child(elapsed(now.saturating_sub(task.started_at))),
            )
    });
    Some(
        div()
            .id("background-tasks")
            .role(Role::Region)
            .aria_label("Background tasks")
            .mb(px(6.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(8.))
            .text_size(px(fs_sm()))
            .children(rows)
            .children(cut_off),
    )
}
