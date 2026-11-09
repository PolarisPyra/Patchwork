//! Settings → Scheduled (`agents/ScheduledPane.tsx`): prompts that start a task at a fixed time,
//! in a project or in a chat. The app runs them on a timer of its own while it is open.

use std::sync::Arc;

use chrono::{Local, TimeZone};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, SharedString, Window, div, prelude::*, px,
};
use patchwork_server::provider::AgentKind;
use patchwork_server::scheduled::{RunOutcome, Schedule, ScheduledRun, ScheduledTask, Weekday};
use patchwork_server::system::now_ms;
use patchwork_server::threads::AccessMode;

use crate::settings::controls::{FieldKind, alert, chip, choice, select_box, switch};
use crate::settings::{SettingsDialog, SettingsEvent};
use crate::ui::theme::{FS_SM, FS_XS, Theme, theme};
use crate::ui::{AGENT_KINDS, BtnKind, agent_label, btn, icon, icon_button, tooltip};

const MODES: [(AccessMode, &str); 4] = [
    (AccessMode::Supervised, "Supervised"),
    (AccessMode::AutoAcceptEdits, "Auto-accept edits"),
    (AccessMode::Auto, "Auto"),
    (AccessMode::FullAccess, "Full access"),
];

const DAYS: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

/// The kinds of schedule the Repeats menu offers.
#[derive(Clone, Copy, PartialEq)]
enum Repeats {
    Hourly,
    Daily,
    Weekdays,
    Weekly,
}

const REPEATS: [(Repeats, &str); 4] = [
    (Repeats::Hourly, "Every few hours"),
    (Repeats::Daily, "Every day"),
    (Repeats::Weekdays, "Weekdays"),
    (Repeats::Weekly, "Days of the week"),
];

/// The names of the editor's fields start with this.
const PREFIX: &str = "sched-";

/// What a choice in one of the editor's menus does to the task.
type Pick = Box<dyn Fn(&mut ScheduledTask)>;

#[derive(Default)]
pub struct ScheduledState {
    tasks: Option<Vec<ScheduledTask>>,
    asked: bool,
    /// The task being edited, or a new one with an empty id.
    editing: Option<ScheduledTask>,
    query: String,
    error: Option<String>,
}

fn repeats(schedule: &Schedule) -> Repeats {
    match schedule {
        Schedule::Hourly { .. } => Repeats::Hourly,
        Schedule::Daily { .. } => Repeats::Daily,
        Schedule::Weekdays { .. } => Repeats::Weekdays,
        Schedule::Weekly { .. } => Repeats::Weekly,
    }
}

/// The time of day a schedule runs at; `09:00` for one that runs every few hours.
fn time_of(schedule: &Schedule) -> &str {
    match schedule {
        Schedule::Daily { time } | Schedule::Weekdays { time } | Schedule::Weekly { time, .. } => {
            time
        }
        Schedule::Hourly { .. } => "09:00",
    }
}

/// What a schedule says in a list row, e.g. `Weekdays at 09:00`.
fn describe_schedule(schedule: &Schedule) -> String {
    match schedule {
        Schedule::Hourly { interval_hours: 1 } => "Every hour".into(),
        Schedule::Hourly { interval_hours } => format!("Every {interval_hours} hours"),
        Schedule::Daily { time } => format!("Daily at {time}"),
        Schedule::Weekdays { time } => format!("Weekdays at {time}"),
        Schedule::Weekly { days, time } => {
            let names = days
                .iter()
                .map(|d| d.short())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{} at {time}",
                if names.is_empty() {
                    "Every day"
                } else {
                    &names
                }
            )
        }
    }
}

/// When a task last or next runs, in local time: `today 09:00`, `tomorrow 09:00`, `Tue 09:00`.
fn when_label(at: Option<i64>) -> String {
    let Some(at) = at.and_then(|ms| Local.timestamp_millis_opt(ms).single()) else {
        return "not yet".into();
    };
    let days = (at.date_naive() - Local::now().date_naive()).num_days();
    let day = match days {
        -1 => "yesterday".to_string(),
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        _ => at.format("%a").to_string(),
    };
    format!("{day} {}", at.format("%H:%M"))
}

fn outcome(run: &ScheduledRun, t: &Theme) -> (&'static str, gpui::Hsla) {
    match run.outcome {
        RunOutcome::Started => ("Ran", t.accent),
        RunOutcome::Failed => ("Failed", t.danger),
        RunOutcome::Skipped => ("Skipped", t.text_subtle),
    }
}

/// A task's newest runs as dots, oldest to newest, then the newest one spelled out (`RunHistory`).
fn run_history(task: &ScheduledTask, t: &Theme) -> AnyElement {
    let runs = &task.recent_runs;
    let Some(newest) = runs.first() else {
        return div().child("Has not run yet").into_any_element();
    };
    let title = runs
        .iter()
        .map(|r| {
            let verb = outcome(r, t).0.to_lowercase();
            [Some(when_label(Some(r.at))), Some(verb), r.detail.clone()]
                .into_iter()
                .flatten()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    div()
        .id(SharedString::from(format!("sched-runs-{}", task.id)))
        .flex()
        .min_w_0()
        .items_center()
        .gap(px(6.))
        .tooltip(tooltip(title))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(2.))
                .children(
                    runs.iter()
                        .rev()
                        .map(|r| div().size(px(6.)).rounded_full().bg(outcome(r, t).1)),
                ),
        )
        .child(div().truncate().child(format!(
            "{} {} · {} recent {}",
            outcome(newest, t).0,
            when_label(Some(newest.at)),
            runs.len(),
            if runs.len() == 1 { "run" } else { "runs" }
        )))
        .into_any_element()
}

/// A blank task, with the settings a new one starts from.
fn blank(agent: AgentKind) -> ScheduledTask {
    ScheduledTask {
        id: String::new(),
        name: String::new(),
        prompt: String::new(),
        schedule: Schedule::Daily {
            time: "09:00".into(),
        },
        project_id: None,
        agent,
        model: None,
        access_mode: AccessMode::Supervised,
        enabled: true,
        last_run_at: None,
        next_run_at: None,
        created_at: now_ms() as i64,
        recent_runs: Vec::new(),
    }
}

/// A form field: a small label above its control (`Field`).
fn labelled(label: &'static str, control: impl IntoElement, t: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(FS_XS))
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.text_muted)
                .child(label),
        )
        .child(control)
}

impl SettingsDialog {
    fn list_scheduled(&mut self, cx: &mut Context<Self>) {
        let server = Arc::clone(&self.server);
        self.run(
            move || server.store.scheduled_tasks(),
            |d, tasks, _| d.scheduled.tasks = Some(tasks),
            cx,
        );
    }

    /// Runs `work` off the main thread, shows its error, then lists the tasks again.
    fn scheduled_run(
        &mut self,
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.scheduled.error = None;
        self.run(
            work,
            |d, out, cx| {
                if let Err(e) = out {
                    d.scheduled.error = Some(e);
                }
                d.list_scheduled(cx);
            },
            cx,
        );
    }

    fn edit_task(&mut self, task: ScheduledTask, cx: &mut Context<Self>) {
        self.drop_fields(PREFIX);
        self.scheduled.query.clear();
        self.scheduled.editing = Some(task);
        cx.notify();
    }

    fn close_task(&mut self, cx: &mut Context<Self>) {
        self.drop_fields(PREFIX);
        self.scheduled.editing = None;
        cx.notify();
    }

    fn save_task(&mut self, cx: &mut Context<Self>) {
        let Some(task) = self.scheduled.editing.clone() else {
            return;
        };
        let server = Arc::clone(&self.server);
        self.scheduled.error = None;
        self.run(
            move || server.save_scheduled_task(task),
            |d, out, cx| match out {
                Ok(_) => {
                    d.close_task(cx);
                    d.list_scheduled(cx);
                }
                Err(e) => d.scheduled.error = Some(e),
            },
            cx,
        );
    }

    /// Changes the task being edited.
    fn patch_task(&mut self, change: impl FnOnce(&mut ScheduledTask), cx: &mut Context<Self>) {
        if let Some(task) = self.scheduled.editing.as_mut() {
            change(task);
        }
        cx.notify();
    }

    /// Opens a menu of `items` for the editor's select named `key`.
    fn task_menu(
        &mut self,
        key: &'static str,
        label: &'static str,
        items: Vec<(String, bool, Pick)>,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.entity().downgrade();
        let items = items
            .into_iter()
            .map(|(text, checked, pick)| {
                choice(weak.clone(), text, checked, move |d, cx| {
                    d.menu = None;
                    d.patch_task(|task| pick(task), cx);
                })
            })
            .collect();
        self.toggle_menu(key, label, items, e.position(), window, cx);
    }
}

/// The form for one task (`TaskEditor`).
fn editor(
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
                )
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
                )
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
                )
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
                                        d.patch_task(|task| task.enabled = !enabled, cx)
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

fn task_card(
    task: &ScheduledTask,
    d: &SettingsDialog,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
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
                        .child(meta("calendar-blank", describe_schedule(&task.schedule)))
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
                                d.scheduled.error = out.err();
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
                        let task = ScheduledTask {
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
    let default_agent = d
        .settings
        .agents
        .first()
        .copied()
        .unwrap_or(AgentKind::Claude);
    let tasks = d.scheduled.tasks.clone();
    let cards: Vec<_> = tasks
        .iter()
        .flatten()
        .map(|task| task_card(task, d, cx))
        .collect();
    let none = tasks.as_ref().is_some_and(|t| t.is_empty()) && d.scheduled.editing.is_none();
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
