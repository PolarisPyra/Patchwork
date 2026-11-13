//! Scheduled tasks as Settings holds them: how a schedule reads, and saving, running and removing one.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use chrono::{Local, TimeZone};
use gpui::{AnyElement, ClickEvent, Div, FontWeight, SharedString, Window, div, prelude::*, px};
use patchwork_server::provider::AgentKind;
use patchwork_server::scheduled::{RunOutcome, Schedule, Run, Task, Weekday};
use patchwork_server::system::now_ms;
use patchwork_server::threads::AccessMode;

use crate::settings::controls::choice;
use crate::settings::SettingsDialog;
use crate::ui::theme::{FS_XS, Theme};
use crate::ui::tooltip;

pub(super) const MODES: [(AccessMode, &str); 4] = [
    (AccessMode::Supervised, "Supervised"),
    (AccessMode::AutoAcceptEdits, "Auto-accept edits"),
    (AccessMode::Auto, "Auto"),
    (AccessMode::FullAccess, "Full access"),
];

pub(super) const DAYS: [Weekday; 7] = [
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
pub(super) enum Repeats {
    Hourly,
    Daily,
    Weekdays,
    Weekly,
}

pub(super) const REPEATS: [(Repeats, &str); 4] = [
    (Repeats::Hourly, "Every few hours"),
    (Repeats::Daily, "Every day"),
    (Repeats::Weekdays, "Weekdays"),
    (Repeats::Weekly, "Days of the week"),
];

/// The names of the editor's fields start with this.
const PREFIX: &str = "sched-";

/// What a choice in one of the editor's menus does to the task.
pub(super) type Pick = Box<dyn Fn(&mut Task)>;

#[derive(Default)]
pub struct ScheduledState {
    pub(super) tasks: Option<Vec<Task>>,
    pub(super) asked: bool,
    /// The task being edited, or a new one with an empty id.
    pub(super) editing: Option<Task>,
    pub(super) query: String,
    pub(super) error: Option<String>,
    /// A save is on its way, so a second press of Save does not make the task twice.
    pub(super) saving: bool,
}

pub(super) fn repeats(schedule: &Schedule) -> Repeats {
    match schedule {
        Schedule::Hourly { .. } => Repeats::Hourly,
        Schedule::Daily { .. } => Repeats::Daily,
        Schedule::Weekdays { .. } => Repeats::Weekdays,
        Schedule::Weekly { .. } => Repeats::Weekly,
    }
}

/// The time of day a schedule runs at; `09:00` for one that runs every few hours.
pub(super) fn time_of(schedule: &Schedule) -> &str {
    match schedule {
        Schedule::Daily { time } | Schedule::Weekdays { time } | Schedule::Weekly { time, .. } => {
            time
        }
        Schedule::Hourly { .. } => "09:00",
    }
}

/// When a task last or next runs, in local time: `today 09:00`, `tomorrow 09:00`, `Tue 09:00`.
pub(super) fn when_label(at: Option<i64>) -> String {
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

fn outcome(run: &Run, t: &Theme) -> (&'static str, gpui::Hsla) {
    match run.outcome {
        RunOutcome::Started => ("Ran", t.accent),
        RunOutcome::Failed => ("Failed", t.danger),
        RunOutcome::Skipped => ("Skipped", t.text_subtle),
    }
}

/// A task's newest runs as dots, oldest to newest, then the newest one spelled out (`RunHistory`).
pub(super) fn run_history(task: &Task, t: &Theme) -> AnyElement {
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
pub(super) fn blank(agent: AgentKind) -> Task {
    Task {
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
pub(super) fn labelled(label: &'static str, control: impl IntoElement, t: &Theme) -> Div {
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
    pub(super) fn list_scheduled(&mut self, cx: &mut Context<Self>) {
        let server = Arc::clone(&self.server);
        self.run(
            move || server.store.scheduled_tasks(),
            |d, tasks, _| d.scheduled.tasks = Some(tasks),
            cx,
        );
    }

    /// Runs `work` off the main thread, shows its error, then lists the tasks again.
    pub(super) fn scheduled_run(
        &mut self,
        work: impl FnOnce() -> Result<(), patchwork_server::Error> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.scheduled.error = None;
        self.run(
            work,
            |d, out, cx| {
                if let Err(e) = out {
                    d.scheduled.error = Some(e.to_string());
                }
                d.list_scheduled(cx);
            },
            cx,
        );
    }

    pub(super) fn edit_task(&mut self, task: Task, cx: &mut Context<Self>) {
        self.drop_fields(PREFIX);
        self.scheduled.query.clear();
        self.scheduled.editing = Some(task);
        cx.notify();
    }

    /// Escape closes the open task, unless it is saving. False when no task is open.
    pub(in crate::settings) fn cancel_task(&mut self, cx: &mut Context<Self>) -> bool {
        if self.scheduled.editing.is_none() {
            return false;
        }
        if !self.scheduled.saving {
            self.close_task(cx);
        }
        true
    }

    pub(super) fn close_task(&mut self, cx: &mut Context<Self>) {
        self.drop_fields(PREFIX);
        self.scheduled.editing = None;
        cx.notify();
    }

    pub(super) fn save_task(&mut self, cx: &mut Context<Self>) {
        let Some(task) = self.scheduled.editing.clone() else {
            return;
        };
        if std::mem::replace(&mut self.scheduled.saving, true) {
            return;
        }
        let server = Arc::clone(&self.server);
        self.scheduled.error = None;
        self.run(
            move || server.save_scheduled_task(task),
            |d, out, cx| {
                d.scheduled.saving = false;
                match out {
                    Ok(_) => {
                        d.close_task(cx);
                        d.list_scheduled(cx);
                    }
                    Err(e) => d.scheduled.error = Some(e.to_string()),
                }
            },
            cx,
        );
    }

    /// Changes the task being edited.
    pub(super) fn patch_task(&mut self, change: impl FnOnce(&mut Task), cx: &mut Context<Self>) {
        if let Some(task) = self.scheduled.editing.as_mut() {
            change(task);
        }
        cx.notify();
    }

    /// Opens a menu of `items` for the editor's select named `key`.
    pub(super) fn task_menu(
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
