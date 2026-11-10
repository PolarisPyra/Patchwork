//! Tasks the agent started that are still running after its reply ended.
//! They keep the turn going, so the user can follow them and stop one from here. A task the agent
//! has no way to stop has no stop button.

// Rust guideline compliant 2026-10-09

use gpui::{Div, FontWeight, Role, SharedString, Stateful, div, prelude::*, px, relative};
use patchwork_server::threads::{BackgroundTask, TaskStatus};

use crate::ui::theme::{FS_SM, RADIUS, Theme};
use crate::ui::{button, icon, now_ms, spinner, tooltip};
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
    fn stop_background(&mut self, tool_id: &str, cx: &mut Context<Self>) {
        let Some(thread) = self.thread.as_ref().map(|t| t.id.clone()) else {
            return;
        };
        if let Err(e) = self.server.stop_subagent(&thread, tool_id) {
            self.error = Some(e.to_string());
        }
        cx.notify();
    }
}

pub fn background_tasks(
    w: &mut Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<Stateful<Div>> {
    // A running subagent is listed in the lineage card, so not here as well.
    let in_lineage = super::lineage::running_ids(w);
    let live: Vec<BackgroundTask> = w
        .thread
        .as_ref()?
        .messages
        .iter()
        .flat_map(|m| &m.background)
        .filter(|task| task.status.is_live() && !in_lineage.contains(&task.tool_id))
        .cloned()
        .collect();
    if live.is_empty() {
        return None;
    }
    w.tick_each_second(cx);
    let now = now_ms();
    let hover = t.bg_hover;
    let rows = live.into_iter().enumerate().map(|(i, task)| {
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
        let stop = task.task_id.is_some().then(|| {
            let tool_id = task.tool_id.clone();
            button(
                SharedString::from(format!("stop-background-{i}")),
                format!("Stop background command {}", task.description),
            )
            .tooltip(tooltip("Stop background command"))
            .flex_none()
            .size(px(24.))
            .justify_center()
            .rounded(px(RADIUS))
            .text_color(t.danger)
            .hover(move |s| s.bg(hover))
            .child(icon("square-fill", 10.))
            .on_click(cx.listener(move |w, _, _, cx| w.stop_background(&tool_id, cx)))
        });
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
            .children(stop)
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
            .text_size(px(FS_SM))
            .children(rows),
    )
}
