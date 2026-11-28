//! The scheduled tasks pane drawn: each task as a card, and the list of them.

// Rust guideline compliant 2026-10-09

use gpui::component::button::Button;
use gpui::component::button::ButtonVariants as _;
use gpui::component::Sizable as _;
use patchwork_views_logos::agent_label;
use std::sync::Arc;

use gpui::{AnyElement, FontWeight, SharedString, Window, div, prelude::*, px};
use patchwork_client::local::core::scheduled::Task;

use crate::settings::controls::{Section, alert, switch};
use crate::settings::{SettingsDialog, SettingsEvent};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{fs_sm, fs_xs, radius_lg, theme};

use patchwork_design_icons::icon;
use super::editor::{editor};
use super::tasks::{blank, run_history, when_label};

fn task_card(task: &Task, d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let place = match &task.project_id {
        Some(id) => d
            .settings
            .projects
            .iter()
            .find(|p| p.id == *id)
            .map_or_else(|| "Project".to_string(), |p| p.name.clone()),
        None if task.agent_id.is_some() => "Agent's own folder".to_string(),
        None => "Scratch chat".to_string(),
    };
    let id = task.id.clone();
    let (run_id, toggle, remove, edit) = (id.clone(), task.clone(), id.clone(), task.clone());
    let enabled = task.enabled;
    let meta = |icon_name: IconName, text: String| {
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
                .child(icon(IconName::Clock, 16.)),
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
                                    .text_size(px(fs_xs()))
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
                        .text_size(px(fs_sm()))
                        .text_color(t.text_muted)
                        .child(meta(
                            IconName::CalendarBlank,
                            patchwork_client::local::core::scheduled::describe_task(task),
                        ))
                        .child(meta(
                            if task.project_id.is_some() {
                                IconName::FolderSimple
                            } else {
                                IconName::ChatCircle
                            },
                            place,
                        ))
                        .child(
                            task.agent_id
                                .as_ref()
                                .and_then(|id| d.scheduled.agents.iter().find(|b| &b.id == id))
                                .map_or_else(
                                    || agent_label(task.agent).to_string(),
                                    |b| b.name.clone(),
                                ),
                        ),
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
                        .text_size(px(fs_xs()))
                        .text_color(t.text_subtle)
                        .child(run_history(task, &t))
                        .when(enabled && task.trigger.is_schedule(), |row| {
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
                    Button::new(SharedString::from(format!("sched-run-{id}")))
                        .small()
                        .accessibility_label(format!("Run {} now", task.name))
                        .px(px(10.))
                        .gap(px(6.))
                        .child(icon(IconName::Play, 13.))
                        .child("Run")
                        .on_click(cx.listener(move |d, _, _, cx| {
                            let (core, id) = (Arc::clone(&d.core), run_id.clone());
                            d.scheduled.error = None;
                            d.run(
                                move || core.run_scheduled_task(&id).map(drop),
                                |d, out, cx| {
                                    d.scheduled.error = out.err().map(|e| e.to_string());
                                    cx.emit(SettingsEvent::ChatsChanged);
                                    d.list_scheduled(cx);
                                },
                                cx,
                            );
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("sched-edit-{id}")))
                        .small()
                        .accessibility_label(format!("Edit {}", task.name))
                        .px(px(10.))
                        .gap(px(6.))
                        .child(icon(IconName::PencilSimple, 13.))
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
                        let core = Arc::clone(&d.core);
                        let task = Task {
                            enabled: !enabled,
                            ..toggle.clone()
                        };
                        d.scheduled_run(move || core.save_scheduled_task(task).map(drop), cx);
                    })),
                )
                .child(
                    Button::new(SharedString::from(format!("sched-delete-{id}")))
                        .ghost()
                        .small()
                        .icon(IconName::Trash)
                        .accessibility_label(format!("Delete {}", task.name))
                        .on_click(cx.listener(move |d, _, _, cx| {
                            let (core, id) = (Arc::clone(&d.core), remove.clone());
                            if d.scheduled.editing.as_ref().is_some_and(|e| e.id == id) {
                                d.close_task(cx);
                            }
                            d.scheduled_run(move || core.remove_scheduled_task(&id), cx);
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
    let actions = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            Button::new("sched-refresh")
                .small()
                .accessibility_label("Refresh")
                .px(px(10.))
                .gap(px(6.))
                .child(icon(IconName::ArrowsClockwise, 13.))
                .child("Refresh")
                .on_click(cx.listener(|d, _, _, cx| d.list_scheduled(cx))),
        )
        .child(
            Button::new("sched-new")
                .small()
                .accessibility_label("New task")
                .px(px(10.))
                .gap(px(6.))
                .child(icon(IconName::Plus, 13.))
                .child("New task")
                .on_click(cx.listener(move |d, _, _, cx| d.edit_task(blank(default_agent), cx))),
        );
    let empty_state = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(8.))
        .rounded(px(radius_lg()))
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
                .child(icon(IconName::Clock, 20.)),
        )
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child("No scheduled tasks yet"),
        );

    div()
        .flex()
        .flex_col()
        .children(
            d.scheduled
                .error
                .clone()
                .map(|e| alert("scheduled-error", e, t.danger).pt_0().pb(px(12.))),
        )
        .children(form)
        .child(
            Section::new("Scheduled tasks")
                .action(actions)
                .bare()
                .child(div().flex().flex_col().gap(px(8.)).children(cards))
                .when(none, |s| s.child(empty_state)),
        )
        .into_any_element()
}
