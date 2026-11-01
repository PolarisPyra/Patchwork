//! Scheduled tasks: a prompt that starts a task on a schedule, the way Codex and Claude desktop
//! run theirs. The schedule is one of a fixed few shapes, so its next run can be worked out from
//! the local clock without a cron parser.
//!
//! When a schedule next runs, and what to do about a run that was missed.

use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::orchestration::{AgentEvent, AgentEventEnvelope};
use crate::provider::AgentKind;
use crate::scheduled;
use crate::system::now_ms;
use crate::threads::{AccessMode, NewThread, Thread};
use crate::Server;

/// A scheduled task: a prompt that starts a task at a fixed time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "scheduled.ts")]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub schedule: Schedule,
    /// The project to run in; None starts a chat in a scratch folder.
    pub project_id: Option<String>,
    pub agent: AgentKind,
    pub model: Option<String>,
    pub access_mode: AccessMode,
    pub enabled: bool,
    /// When the task last started, in Unix milliseconds.
    pub last_run_at: Option<i64>,
    /// When it next starts, in Unix milliseconds.
    pub next_run_at: Option<i64>,
    pub created_at: i64,
    /// The newest few runs, newest first.
    #[serde(default)]
    pub recent_runs: Vec<ScheduledRun>,
}

/// One time a scheduled task's slot came round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "scheduled.ts")]
#[serde(rename_all = "camelCase")]
pub struct ScheduledRun {
    /// When it started, or for a skipped run the slot it missed, in Unix milliseconds.
    pub at: i64,
    pub thread_id: Option<String>,
    pub outcome: RunOutcome,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "scheduled.ts")]
#[serde(rename_all = "lowercase")]
pub enum RunOutcome {
    Started,
    Failed,
    /// Its time went by more than a day ago, so it was not run late.
    Skipped,
}

/// How often a scheduled task runs. The shapes Codex's own tasks use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "scheduled.ts")]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Schedule {
    Hourly {
        interval_hours: u32,
    },
    /// Every day at `time`, `HH:MM` in local time.
    Daily {
        time: String,
    },
    /// Monday to Friday at `time`.
    Weekdays {
        time: String,
    },
    /// On each of `days` at `time`.
    Weekly {
        days: Vec<Weekday>,
        time: String,
    },
}

/// A day of the week, written as Codex's schedules write it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "scheduled.ts")]
pub enum Weekday {
    #[serde(rename = "MO")]
    Mon,
    #[serde(rename = "TU")]
    Tue,
    #[serde(rename = "WE")]
    Wed,
    #[serde(rename = "TH")]
    Thu,
    #[serde(rename = "FR")]
    Fri,
    #[serde(rename = "SA")]
    Sat,
    #[serde(rename = "SU")]
    Sun,
}

impl Weekday {
    pub fn from_chrono(day: chrono::Weekday) -> Self {
        match day {
            chrono::Weekday::Mon => Weekday::Mon,
            chrono::Weekday::Tue => Weekday::Tue,
            chrono::Weekday::Wed => Weekday::Wed,
            chrono::Weekday::Thu => Weekday::Thu,
            chrono::Weekday::Fri => Weekday::Fri,
            chrono::Weekday::Sat => Weekday::Sat,
            chrono::Weekday::Sun => Weekday::Sun,
        }
    }

    /// `Mon`, `Tue`, … for a list row.
    pub fn short(self) -> &'static str {
        match self {
            Weekday::Mon => "Mon",
            Weekday::Tue => "Tue",
            Weekday::Wed => "Wed",
            Weekday::Thu => "Thu",
            Weekday::Fri => "Fri",
            Weekday::Sat => "Sat",
            Weekday::Sun => "Sun",
        }
    }
}

impl Server {
    /// Starts a task in a new thread for `task`, and returns the thread.
    ///
    /// A task whose project no longer exists runs as a chat, so a deleted project does not stop
    /// the schedule without saying so.
    pub fn start_scheduled_task(
        self: &Arc<Self>,
        task: &crate::scheduled::ScheduledTask,
    ) -> Result<Thread, String> {
        let project_id = task.project_id.clone().filter(|id| {
            self.store
                .settings()
                .projects
                .iter()
                .any(|project| &project.id == id)
        });
        let thread = self.create_thread(NewThread {
            id: None,
            project_id,
            agent: task.agent,
            model: task.model.clone(),
            access_mode: task.access_mode,
            effort: None,
            compact_window: None,
            plan_mode: false,
        })?;
        let thread = self.store.thread(&thread.id).unwrap_or(thread);
        self.emit(AgentEventEnvelope {
            thread_id: thread.id.clone(),
            event: AgentEvent::ThreadCreated {
                thread: thread.clone(),
            },
        });
        self.send_message(thread.id.clone(), task.prompt.clone(), Vec::new(), false)?;
        // Named after the task, not after its prompt: the first message is what the schedule says,
        // and the task's own name is what the list should show. This comes after the message so
        // the naming a new thread gets from its first line cannot take the name back.
        self.store
            .update_thread(&thread.id, |t| t.title = task.name.clone())?;
        self.emit(AgentEventEnvelope {
            thread_id: thread.id.clone(),
            event: AgentEvent::TitleUpdated {
                title: task.name.clone(),
            },
        });
        let thread = self.store.thread(&thread.id).unwrap_or(thread);
        Ok(thread)
    }

    /// Starts one scheduled task now, whatever its schedule says.
    pub fn run_scheduled_task(self: &Arc<Self>, id: &str) -> Result<Thread, String> {
        let task = self
            .store
            .scheduled_tasks()
            .into_iter()
            .find(|task| task.id == id)
            .ok_or("The scheduled task no longer exists")?;
        let thread = self.start_scheduled_task(&task)?;
        let now = now_ms() as i64;
        let next = scheduled::next_run_at(&task.schedule, chrono::Local::now());
        self.store
            .scheduled_task_ran(&task.id, now, next, Ok(thread.id.as_str()))?;
        Ok(thread)
    }

    /// Whether a reply is under way in a thread of `project_id`. Chats have no project and never
    /// collide, since each has a scratch folder of its own.
    fn busy_in_project(&self, project_id: Option<&str>) -> bool {
        let Some(project_id) = project_id else {
            return false;
        };
        self.runner.running().iter().any(|thread_id| {
            self.store
                .thread(thread_id)
                .is_some_and(|thread| thread.project_id.as_deref() == Some(project_id))
        })
    }

    /// Starts every scheduled task whose time has come, and returns their names. Called on a
    /// timer by the desktop shell; a task that fails to start is logged and recorded. A task whose
    /// project has a reply under way waits for a later tick, and a slot that went by too long ago
    /// is skipped.
    pub fn run_due_scheduled_tasks(self: &Arc<Self>) -> Vec<String> {
        let now = now_ms() as i64;
        let tasks = self.store.scheduled_tasks();
        let mut started = Vec::new();
        for task in &tasks {
            let next = || scheduled::next_run_at(&task.schedule, chrono::Local::now());
            match scheduled::action(task, now) {
                scheduled::Action::Wait => {}
                scheduled::Action::Skip => {
                    log::info!("scheduled task {} missed its time; skipped", task.name);
                    if let Err(e) = self.store.scheduled_task_skipped(&task.id, next()) {
                        log::warn!("scheduled task {}: {e}", task.id);
                    }
                }
                // It stays due, so the next tick tries again.
                scheduled::Action::Run if self.busy_in_project(task.project_id.as_deref()) => {}
                scheduled::Action::Run => match self.start_scheduled_task(task) {
                    Ok(thread) => {
                        let ran = self.store.scheduled_task_ran(
                            &task.id,
                            now,
                            next(),
                            Ok(thread.id.as_str()),
                        );
                        if let Err(e) = ran {
                            log::warn!("scheduled task {}: {e}", task.id);
                        }
                        started.push(task.name.clone());
                    }
                    Err(e) => {
                        log::warn!("scheduled task {} did not start: {e}", task.name);
                        // Record the attempt so a task that cannot start does not retry every tick.
                        let _ =
                            self.store
                                .scheduled_task_ran(&task.id, now, next(), Err(e.as_str()));
                    }
                },
            }
        }
        started
    }
}

/// A schedule's time of day, `HH:MM`, as a `NaiveTime`.
fn time_of_day(time: &str) -> Option<NaiveTime> {
    let (hour, minute) = time.split_once(':')?;
    let hour: u32 = hour.trim().parse().ok()?;
    let minute: u32 = minute.trim().parse().ok()?;
    NaiveTime::from_hms_opt(hour, minute, 0)
}

/// One day after `day`, or the next day `schedule` allows.
fn next_day(day: NaiveDate, schedule: &Schedule) -> NaiveDate {
    let mut day = day.succ_opt().unwrap_or(day);
    // A weekly schedule with no days would otherwise never come round; treat it as any day.
    for _ in 0..7 {
        if day_allowed(schedule, day) {
            break;
        }
        day = day.succ_opt().unwrap_or(day);
    }
    day
}

/// The first run strictly after `from`, in local time. `None` when the schedule is malformed
/// (a time that is not `HH:MM`), which the editor refuses to save.
pub fn next_after(schedule: &Schedule, from: NaiveDateTime) -> Option<NaiveDateTime> {
    match schedule {
        Schedule::Hourly { interval_hours } => {
            let hours = (*interval_hours).clamp(1, 24 * 7) as i64;
            Some(from + Duration::hours(hours))
        }
        Schedule::Daily { time } => {
            let at = time_of_day(time)?;
            let today = from.date().and_time(at);
            if today > from {
                Some(today)
            } else {
                Some(from.date().succ_opt()?.and_time(at))
            }
        }
        Schedule::Weekdays { time } | Schedule::Weekly { time, .. } => {
            let at = time_of_day(time)?;
            let today = from.date().and_time(at);
            if today > from && day_allowed(schedule, from.date()) {
                return Some(today);
            }
            let mut day = from.date();
            for _ in 0..8 {
                day = next_day(day, schedule);
                if day_allowed(schedule, day) {
                    return Some(day.and_time(at));
                }
            }
            None
        }
    }
}

/// Whether `schedule` may run on `day`.
fn day_allowed(schedule: &Schedule, day: NaiveDate) -> bool {
    match schedule {
        Schedule::Hourly { .. } | Schedule::Daily { .. } => true,
        Schedule::Weekdays { .. } => {
            !matches!(day.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun)
        }
        // A weekly schedule with no days would never come round; treat it as every day.
        Schedule::Weekly { days, .. } => {
            days.is_empty() || days.contains(&Weekday::from_chrono(day.weekday()))
        }
    }
}

/// The first run after `now`, as Unix milliseconds. A time the clocks jump over runs an hour
/// later, once the clock has moved past it.
pub fn next_run_at(schedule: &Schedule, now: DateTime<Local>) -> Option<i64> {
    let next = next_after(schedule, now.naive_local())?;
    Local
        .from_local_datetime(&next)
        .earliest()
        .or_else(|| {
            Local
                .from_local_datetime(&(next + Duration::hours(1)))
                .earliest()
        })
        .map(|at| at.timestamp_millis())
}

/// A one-line description of the schedule for a list row, e.g. `Weekdays at 09:00`.
pub fn describe(schedule: &Schedule) -> String {
    match schedule {
        Schedule::Hourly { interval_hours } => {
            if *interval_hours == 1 {
                "Every hour".to_string()
            } else {
                format!("Every {interval_hours} hours")
            }
        }
        Schedule::Daily { time } => format!("Daily at {time}"),
        Schedule::Weekdays { time } => format!("Weekdays at {time}"),
        Schedule::Weekly { days, time } => {
            let names: Vec<&str> = days.iter().map(|day| day.short()).collect();
            format!("{} at {time}", names.join(", "))
        }
    }
}

/// How late a slot may still run: one missed while the app was closed or asleep runs when it
/// comes back, unless it went by more than this long ago. A week-old "what changed today" is
/// worth nothing.
pub const CATCH_UP_WITHIN_MS: i64 = 24 * 60 * 60 * 1000;

/// What to do with a task on a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Its slot has come, recently enough to run.
    Run,
    /// Its slot went by longer ago than `CATCH_UP_WITHIN_MS`: move on to the next one.
    Skip,
    /// Not due, or switched off.
    Wait,
}

/// What to do with `task` at `now_ms`. A task with no next run waits, so saving one does not
/// fire it.
pub fn action(task: &ScheduledTask, now_ms: i64) -> Action {
    match task.next_run_at {
        Some(at) if task.enabled && at <= now_ms => {
            if now_ms - at <= CATCH_UP_WITHIN_MS {
                Action::Run
            } else {
                Action::Skip
            }
        }
        Some(_) | None => Action::Wait,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").expect("a date and time")
    }

    fn next(schedule: &Schedule, from: &str) -> String {
        next_after(schedule, at(from))
            .expect("a next run")
            .format("%Y-%m-%d %H:%M")
            .to_string()
    }

    #[test]
    fn hourly_steps_by_its_interval() {
        let hourly = Schedule::Hourly { interval_hours: 6 };
        assert_eq!(next(&hourly, "2026-10-28 10:00"), "2026-10-28 16:00");
        // A zero interval still fires rather than looping forever.
        let zero = Schedule::Hourly { interval_hours: 0 };
        assert_eq!(next(&zero, "2026-10-28 10:00"), "2026-10-28 11:00");
    }

    #[test]
    fn daily_takes_today_when_the_time_is_still_ahead() {
        let daily = Schedule::Daily {
            time: "09:00".into(),
        };
        assert_eq!(next(&daily, "2026-10-28 08:00"), "2026-10-28 09:00");
        assert_eq!(next(&daily, "2026-10-28 09:00"), "2026-10-29 09:00");
        assert_eq!(next(&daily, "2026-10-28 23:30"), "2026-10-29 09:00");
    }

    #[test]
    fn weekdays_skip_the_weekend() {
        let weekdays = Schedule::Weekdays {
            time: "09:00".into(),
        };
        // 2026-10-30 is a Friday.
        assert_eq!(next(&weekdays, "2026-10-30 12:00"), "2026-11-02 09:00");
        // Before the time, a weekday keeps the same day.
        assert_eq!(next(&weekdays, "2026-10-30 08:00"), "2026-10-30 09:00");
    }

    #[test]
    fn weekly_waits_for_its_day() {
        let weekly = Schedule::Weekly {
            days: vec![Weekday::Mon],
            time: "09:00".into(),
        };
        assert_eq!(next(&weekly, "2026-10-28 10:00"), "2026-11-02 09:00");
        // On the day itself, before its time.
        assert_eq!(next(&weekly, "2026-11-02 08:00"), "2026-11-02 09:00");
        let empty = Schedule::Weekly {
            days: vec![],
            time: "09:00".into(),
        };
        assert_eq!(next(&empty, "2026-10-28 10:00"), "2026-10-29 09:00");
    }

    #[test]
    fn a_bad_time_has_no_next_run() {
        let bad = Schedule::Daily {
            time: "9 o'clock".into(),
        };
        assert_eq!(next_after(&bad, at("2026-10-28 10:00")), None);
        let bad_hour = Schedule::Daily {
            time: "25:00".into(),
        };
        assert_eq!(next_after(&bad_hour, at("2026-10-28 10:00")), None);
    }

    #[test]
    fn a_task_runs_when_due_and_skips_a_slot_long_gone() {
        let task = |enabled: bool, next_run_at: Option<i64>| ScheduledTask {
            id: "t".into(),
            name: "n".into(),
            prompt: "p".into(),
            schedule: Schedule::Daily {
                time: "09:00".into(),
            },
            project_id: None,
            agent: crate::provider::AgentKind::Claude,
            model: None,
            access_mode: crate::threads::AccessMode::Supervised,
            enabled,
            last_run_at: None,
            next_run_at,
            created_at: 0,
            recent_runs: Vec::new(),
        };
        let slot = 1_000;
        assert_eq!(action(&task(true, Some(slot)), slot), Action::Run);
        assert_eq!(action(&task(true, Some(slot)), slot - 1), Action::Wait);
        assert_eq!(action(&task(false, Some(slot)), slot), Action::Wait);
        assert_eq!(action(&task(true, None), slot), Action::Wait);
        // Up to a day late still runs; later than that is skipped.
        let day_late = slot + CATCH_UP_WITHIN_MS;
        assert_eq!(action(&task(true, Some(slot)), day_late), Action::Run);
        assert_eq!(action(&task(true, Some(slot)), day_late + 1), Action::Skip);
    }

    /// The JSON the web UI reads and writes, which is what the database holds too.
    #[test]
    fn a_schedule_is_written_in_the_shape_the_ui_reads() {
        let hourly = serde_json::to_string(&Schedule::Hourly { interval_hours: 3 }).expect("json");
        assert_eq!(hourly, r#"{"type":"hourly","intervalHours":3}"#);
        let weekly = serde_json::to_string(&Schedule::Weekly {
            days: vec![Weekday::Mon],
            time: "09:00".into(),
        })
        .expect("json");
        assert_eq!(weekly, r#"{"type":"weekly","days":["MO"],"time":"09:00"}"#);
        let read: Schedule =
            serde_json::from_str(r#"{"type":"daily","time":"07:30"}"#).expect("read");
        assert_eq!(
            read,
            Schedule::Daily {
                time: "07:30".into()
            }
        );
    }

    #[test]
    fn describe_names_each_shape() {
        assert_eq!(
            describe(&Schedule::Hourly { interval_hours: 1 }),
            "Every hour"
        );
        assert_eq!(
            describe(&Schedule::Hourly { interval_hours: 3 }),
            "Every 3 hours"
        );
        assert_eq!(
            describe(&Schedule::Daily {
                time: "07:30".into()
            }),
            "Daily at 07:30"
        );
        assert_eq!(
            describe(&Schedule::Weekly {
                days: vec![Weekday::Mon, Weekday::Fri],
                time: "09:00".into()
            }),
            "Mon, Fri at 09:00"
        );
    }
}
