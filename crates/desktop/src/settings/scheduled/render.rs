//! The scheduled tasks pane drawn: each task as a card, and the list of them.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{AnyElement, FontWeight, SharedString, Window, div, prelude::*, px};
use patchwork_server::scheduled::Task;

use crate::settings::controls::{alert, switch};
use crate::settings::{SettingsDialog, SettingsEvent};
use crate::ui::theme::{FS_SM, FS_XS, theme};
use crate::ui::{BtnKind, agent_label, btn, icon, icon_button, tooltip};
use super::editor::{editor};
use super::state::{blank, run_history, when_label};

fn task_card(task: &Task, d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let place = match &task.project_id {
        Some(id) => d
            .settings
            .projects
            .iter()
            .find(|p| p.id == *id)
            .map_or_else(|| "Project".to_string(), |p| p.name.clone()),
        None => "Scratch chat".to_string(),
    };
    let id = task.id.clone();
    let (run_id, toggle, remove, edit) = (id.clone(), task.clone(), id.clone(), task.clone());
    let enabled = task.enabled;
    let meta = |icon_name: &'static str, text: String| {
        div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(px(4.))
            .child(icon(icon_name, 12.))
            .child(div().truncate().child(text))
    };
    div()
        .flex()
        .min_w_0()
        .items_center()
        .gap(px(12.))
        .border_1()
        .border_color(t.border)
        .bg(t.bg_panel)
        .px(px(12.))
        .py(px(10.))
        .child(
            div()
                .flex()
                .flex_none()
                .size(px(32.))
                .items_center()
                .justify_center()
                .rounded_full()
                .map(|d| {
                    if enabled {
                        d.bg(t.bg_accent_soft).text_color(t.accent)
                    } else {
                        d.bg(t.bg_hover).text_color(t.text_subtle)
                    }
                })
                .child(icon("clock", 16.)),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .flex()
                        .min_w_0()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(task.name.clone()),
                        )
                        .when(!enabled, |row| {
                            row.child(
                                div()
                                    .flex_none()
                                    .rounded_full()
                                    .bg(t.bg_hover)
                                    .px(px(8.))
                                    .text_size(px(FS_XS))
                                    .text_color(t.text_muted)
                                    .child("Paused"),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .min_w_0()
                        .items_center()
                        .gap_x(px(12.))
                        .gap_y(px(2.))
                        .text_size(px(FS_SM))
                        .text_color(t.text_muted)
                        .child(meta(
                            "calendar-blank",
                            patchwork_server::scheduled::describe(&task.schedule),
                        ))
                        .child(meta(
                            if task.project_id.is_some() {
                                "folder-simple"
                            } else {
                                "chat-circle"
                            },
                            place,
                        ))
                        .child(agent_label(task.agent)),
                )
                .child(
                    div()
                        .mt(px(4.))
                        .flex()
                        .flex_wrap()
                        .min_w_0()
                        .items_center()
                        .gap_x(px(12.))
                        .gap_y(px(2.))
                        .text_size(px(FS_XS))
                        .text_color(t.text_subtle)
                        .child(run_history(task, &t))
                        .when(enabled, |row| {
                            row.child(
                                div()
                                    .flex_none()
                                    .child(format!("Next {}", when_label(task.next_run_at))),
                            )
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .child(
                    btn(
                        SharedString::from(format!("sched-run-{id}")),
                        format!("Run {} now", task.name),
                        BtnKind::Plain,
                        false,
                        &t,
                    )
                    .px(px(10.))
                    .gap(px(6.))
                    .tooltip(tooltip("Start this task now"))
                    .child(icon("play", 13.))
                    .child("Run")
                    .on_click(cx.listener(move |d, _, _, cx| {
                        let (server, id) = (Arc::clone(&d.server), run_id.clone());
                        d.scheduled.error = None;
                        d.run(
                            move || server.run_scheduled_task(&id).map(drop),
                            |d, out, cx| {
                                d.scheduled.error = out.err().map(|e| e.to_string());
                                cx.emit(SettingsEvent::ThreadsChanged);
                                d.list_scheduled(cx);
                            },
                            cx,
                        );
                    })),
                )
                .child(
                    btn(
                        SharedString::from(format!("sched-edit-{id}")),
                        format!("Edit {}", task.name),
                        BtnKind::Plain,
                        false,
                        &t,
                    )
                    .px(px(10.))
                    .gap(px(6.))
                    .child(icon("pencil-simple", 13.))
                    .child("Edit")
                    .on_click(cx.listener(move |d, _, _, cx| d.edit_task(edit.clone(), cx))),
                )
                .child(
                    switch(
                        SharedString::from(format!("sched-on-{id}")),
                        format!("{} runs on its schedule", task.name),
                        enabled,
                        &t,
                    )
                    .on_click(cx.listener(move |d, _, _, cx| {
                        let server = Arc::clone(&d.server);
                        let task = Task {
                            enabled: !enabled,
                            ..toggle.clone()
                        };
                        d.scheduled_run(move || server.save_scheduled_task(task).map(drop), cx);
                    })),
                )
                .child(
                    icon_button(
                        SharedString::from(format!("sched-delete-{id}")),
                        format!("Delete {}", task.name),
                        "trash",
                        14.,
                        cx,
                    )
                    .tooltip(tooltip("Delete task"))
                    .on_click(cx.listener(move |d, _, _, cx| {
                        let (server, id) = (Arc::clone(&d.server), remove.clone());
                        if d.scheduled.editing.as_ref().is_some_and(|e| e.id == id) {
                            d.close_task(cx);
                        }
                        d.scheduled_run(move || server.store.remove_scheduled_task(&id), cx);
                    })),
                ),
        )
        .into_any_element()
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    if !std::mem::replace(&mut d.scheduled.asked, true) {
        d.list_scheduled(cx);
    }
    let default_agent = d.settings.default_agent();
    let tasks = d.scheduled.tasks.clone();
    let cards: Vec<_> = tasks
        .iter()
        .flatten()
        .map(|task| task_card(task, d, cx))
        .collect();
    let none = tasks.as_ref().is_some_and(std::vec::Vec::is_empty) && d.scheduled.editing.is_none();
    let form = editor(d, window, cx);

    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .pb(px(12.))
                .child(
                    btn("sched-new", "New task", BtnKind::Plain, false, &t)
                        .px(px(10.))
                        .gap(px(6.))
                        .child(icon("plus", 13.))
                        .child("New task")
                        .on_click(
                            cx.listener(move |d, _, _, cx| d.edit_task(blank(default_agent), cx)),
                        ),
                )
                .child(
                    div()
                        .text_size(px(FS_SM))
                        .text_color(t.text_muted)
                        .child("Tasks run while Patchwork is open."),
                )
                .child(
                    btn("sched-refresh", "Refresh", BtnKind::Plain, false, &t)
                        .ml_auto()
                        .px(px(10.))
                        .gap(px(6.))
                        .child(icon("arrow-clockwise", 13.))
                        .child("Refresh")
                        .on_click(cx.listener(|d, _, _, cx| d.list_scheduled(cx))),
                ),
        )
        .when(!cards.is_empty(), |col| {
            col.child(div().flex().flex_col().gap(px(8.)).children(cards))
        })
        .when(none, |col| {
            col.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .border_1()
                    .border_dashed()
                    .border_color(t.border_strong)
                    .py(px(32.))
                    .child(
                        div()
                            .flex()
                            .size(px(40.))
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(t.bg_accent_soft)
                            .text_color(t.accent)
                            .child(icon("clock", 20.)),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("No scheduled tasks yet"),
                    )
                    .child(
                        div()
                            .text_size(px(FS_SM))
                            .text_color(t.text_muted)
                            .child("Each run opens a new task that starts with your prompt."),
                    ),
            )
        })
        .children(form)
        .children(
            d.scheduled
                .error
                .clone()
                .map(|e| alert("scheduled-error", e, t.danger)),
        )
        .into_any_element()
}
