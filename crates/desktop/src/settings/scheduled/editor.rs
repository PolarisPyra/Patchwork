//! The form for one scheduled task.

// Rust guideline compliant 2026-10-09

use gpui::{ClickEvent, Div, SharedString, Window, div, prelude::*, px};
use patchwork_server::scheduled::{Schedule, Weekday};

use crate::settings::controls::{FieldKind, chip, select_box, switch};
use crate::settings::SettingsDialog;
use crate::ui::theme::{FS_SM, FS_XS, theme};
use crate::ui::{AGENT_KINDS, BtnKind, agent_label, btn, icon, tooltip};
use super::state::{DAYS, MODES, Pick, REPEATS, Repeats, labelled, repeats, time_of};

/// The form for one task (`TaskEditor`).
pub(super) fn editor(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Option<Div> {
    let t = *theme(cx);
    let draft = d.scheduled.editing.clone()?;
    let projects = d.settings.projects.clone();

    let name = d.field(
        "sched-name",
        "Name",
        &draft.name,
        "Weekly report",
        FieldKind {
            live: true,
            ..Default::default()
        },
        |d, text, cx| d.patch_task(|task| task.name = text, cx),
        window,
        cx,
    );
    let prompt = d.field(
        "sched-prompt",
        "Prompt",
        &draft.prompt,
        "The prompt the task starts with.",
        FieldKind {
            live: true,
            lines: true,
            ..Default::default()
        },
        |d, text, cx| d.patch_task(|task| task.prompt = text, cx),
        window,
        cx,
    );

    let kind = repeats(&draft.schedule);
    let at = time_of(&draft.schedule).to_string();
    let repeats_select = select_box(
        "sched-repeats",
        "How often",
        REPEATS
            .iter()
            .find(|(r, _)| *r == kind)
            .map_or("", |(_, l)| l),
        d.menu_open("sched-repeats"),
        &t,
    )
    .on_click(cx.listener({
        let at = at.clone();
        move |d, e: &ClickEvent, window, cx| {
            let items = REPEATS
                .iter()
                .map(|&(r, label)| {
                    let at = at.clone();
                    let pick: Pick = Box::new(move |task| {
                        task.schedule = match r {
                            Repeats::Hourly => Schedule::Hourly { interval_hours: 6 },
                            Repeats::Daily => Schedule::Daily { time: at.clone() },
                            Repeats::Weekdays => Schedule::Weekdays { time: at.clone() },
                            Repeats::Weekly => Schedule::Weekly {
                                days: vec![Weekday::Mon],
                                time: at.clone(),
                            },
                        }
                    });
                    (label.to_string(), r == kind, pick)
                })
                .collect();
            d.drop_fields("sched-every");
            d.drop_fields("sched-at");
            d.task_menu("sched-repeats", "How often", items, e, window, cx);
        }
    }));

    let when = if let Schedule::Hourly { interval_hours } = draft.schedule {
        let hours = d.field(
            "sched-every",
            "Hours between runs",
            &interval_hours.to_string(),
            "",
            FieldKind {
                live: true,
                ..Default::default()
            },
            |d, text, cx| {
                let hours = text.trim().parse::<u32>().unwrap_or(1).clamp(1, 168);
                d.patch_task(
                    |task| {
                        task.schedule = Schedule::Hourly {
                            interval_hours: hours,
                        }
                    },
                    cx,
                );
            },
            window,
            cx,
        );
        labelled(
            "Every",
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
                .child(hours.w(px(56.)))
                .child("hours"),
            &t,
        )
    } else {
        let time = d.field(
            "sched-at",
            "Time of day",
            &at,
            "09:00",
            FieldKind {
                live: true,
                ..Default::default()
            },
            |d, text, cx| {
                // Only a whole `HH:MM` is kept; the field shows the saved time again once left.
                let Ok(time) = chrono::NaiveTime::parse_from_str(text.trim(), "%H:%M") else {
                    return;
                };
                let time = time.format("%H:%M").to_string();
                d.patch_task(
                    |task| match &mut task.schedule {
                        Schedule::Daily { time: at }
                        | Schedule::Weekdays { time: at }
                        | Schedule::Weekly { time: at, .. } => *at = time,
                        Schedule::Hourly { .. } => {}
                    },
                    cx,
                );
            },
            window,
            cx,
        );
        labelled("At", time.w(px(96.)), &t)
    };

    let days = if let Schedule::Weekly { days, .. } = &draft.schedule {
        let chips = DAYS.iter().map(|&day| {
            let on = days.contains(&day);
            chip(
                SharedString::from(format!("sched-day-{}", day.short())),
                day.short(),
                on,
                &t,
            )
            .child(day.short())
            .on_click(cx.listener(move |d, _, _, cx| {
                d.patch_task(
                    |task| {
                        if let Schedule::Weekly { days, .. } = &mut task.schedule {
                            if on {
                                days.retain(|d| *d != day);
                            } else {
                                days.push(day);
                            }
                        }
                    },
                    cx,
                );
            }))
        });
        Some(labelled(
            "On",
            div().flex().items_center().gap(px(2.)).children(chips),
            &t,
        ))
    } else {
        None
    };

    let search = (projects.len() > 4).then(|| {
        let query = d.scheduled.query.clone();
        let field = d.field(
            "sched-find",
            "Find a project",
            &query,
            "Find a project",
            FieldKind {
                live: true,
                ..Default::default()
            },
            |d, text, cx| {
                d.scheduled.query = text;
                cx.notify();
            },
            window,
            cx,
        );
        div()
            .relative()
            .flex()
            .items_center()
            .child(field.w_full().pl(px(26.)))
            .child(
                div()
                    .absolute()
                    .left(px(8.))
                    .text_color(t.text_muted)
                    .child(icon("magnifying-glass", 13.)),
            )
    });
    let place = draft
        .project_id
        .as_ref()
        .and_then(|id| projects.iter().find(|p| p.id == *id))
        .map_or_else(
            || "A chat in a scratch folder".to_string(),
            |p| p.name.clone(),
        );
    let where_select = select_box(
        "sched-where",
        "Project",
        place,
        d.menu_open("sched-where"),
        &t,
    )
    .w_full()
    .on_click(cx.listener({
        let (projects, current) = (projects.clone(), draft.project_id.clone());
        move |d, e: &ClickEvent, window, cx| {
            let query = d.scheduled.query.trim().to_lowercase();
            let scratch: Pick = Box::new(|task| task.project_id = None);
            let items = std::iter::once((
                "A chat in a scratch folder".to_string(),
                current.is_none(),
                scratch,
            ))
            .chain(
                projects
                    .iter()
                    .filter(|p| p.name.to_lowercase().contains(&query))
                    .map(|p| {
                        let id = p.id.clone();
                        let checked = current.as_deref() == Some(id.as_str());
                        let pick: Pick = Box::new(move |task| task.project_id = Some(id.clone()));
                        (p.name.clone(), checked, pick)
                    }),
            )
            .collect();
            d.task_menu("sched-where", "Project", items, e, window, cx);
        }
    }));
    let agent = draft.agent;
    let agent_select = select_box(
        "sched-agent",
        "Agent",
        agent_label(agent),
        d.menu_open("sched-agent"),
        &t,
    )
    .on_click(cx.listener(move |d, e: &ClickEvent, window, cx| {
        let items = AGENT_KINDS
            .iter()
            .map(|&kind| {
                let pick: Pick = Box::new(move |task| {
                    task.agent = kind;
                    task.model = None;
                });
                (agent_label(kind).to_string(), kind == agent, pick)
            })
            .collect();
        d.task_menu("sched-agent", "Agent", items, e, window, cx);
    }));
    let mode = draft.access_mode;
    let mode_select = select_box(
        "sched-access",
        "Access mode",
        MODES
            .iter()
            .find(|(m, _)| *m == mode)
            .map_or("", |(_, l)| l),
        d.menu_open("sched-access"),
        &t,
    )
    .on_click(cx.listener(move |d, e: &ClickEvent, window, cx| {
        let items = MODES
            .iter()
            .map(|&(m, label)| {
                let pick: Pick = Box::new(move |task| task.access_mode = m);
                (label.to_string(), m == mode, pick)
            })
            .collect();
        d.task_menu("sched-access", "Access mode", items, e, window, cx);
    }));
    let path = draft
        .project_id
        .as_ref()
        .and_then(|id| projects.iter().find(|p| p.id == *id))
        .map(|p| p.path.clone());

    let ready = !draft.name.trim().is_empty() && !draft.prompt.trim().is_empty();
    let enabled = draft.enabled;
    Some(
        div()
            .mt(px(12.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_panel)
            .p(px(14.))
            .child(labelled("Name", name, &t))
            .child(labelled("Prompt", prompt, &t))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .gap(px(12.))
                    .child(labelled("Repeats", repeats_select, &t))
                    .child(when)
                    .children(days),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .gap(px(12.))
                    .child(
                        labelled(
                            "Where",
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .children(search)
                                .child(where_select),
                            &t,
                        )
                        .min_w(px(200.))
                        .flex_1(),
                    )
                    .child(labelled("Agent", agent_select, &t))
                    .child(labelled("Access", mode_select, &t)),
            )
            .children(path.map(|path| {
                div()
                    .mt(px(-4.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(FS_XS))
                    .text_color(t.text_subtle)
                    .child(icon("folder-simple", 12.))
                    .child(path)
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .border_t_1()
                    .border_color(t.border)
                    .pt(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(FS_SM))
                            .text_color(t.text_muted)
                            .child("Enabled")
                            .child(
                                switch("sched-enabled", "Task is enabled", enabled, &t).on_click(
                                    cx.listener(move |d, _, _, cx| {
                                        d.patch_task(|task| task.enabled = !enabled, cx);
                                    }),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                btn("sched-cancel", "Cancel", BtnKind::Plain, false, &t)
                                    .child("Cancel")
                                    .on_click(cx.listener(|d, _, _, cx| d.close_task(cx))),
                            )
                            .child(
                                btn("sched-save", "Save task", BtnKind::Primary, !ready, &t)
                                    .child("Save task")
                                    .map(|b| {
                                        if ready {
                                            b.on_click(cx.listener(|d, _, _, cx| d.save_task(cx)))
                                        } else {
                                            b.tooltip(tooltip("Add a name and a prompt"))
                                        }
                                    }),
                            ),
                    ),
            ),
    )
}
