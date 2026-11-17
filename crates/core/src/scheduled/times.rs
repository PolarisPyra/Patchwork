//! When a schedule next runs, how it reads in words, and what a tick does with a task.

// Rust guideline compliant 2026-10-09

use super::types::Task;
use super::types::Weekday;
use super::types::Schedule;
use crate::Error;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use crate::system::new_id;

/// A schedule's time of day, `HH:MM`, as a `NaiveTime`.
pub(super) fn time_of_day(time: &str) -> Option<NaiveTime> {
    let (hour, minute) = time.split_once(':')?;
    let hour: u32 = hour.trim().parse().ok()?;
    let minute: u32 = minute.trim().parse().ok()?;
    NaiveTime::from_hms_opt(hour, minute, 0)
}

/// One day after `day`, or the next day `schedule` allows.
pub(super) fn next_day(day: NaiveDate, schedule: &Schedule) -> NaiveDate {
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
#[must_use]
pub fn next_after(schedule: &Schedule, from: NaiveDateTime) -> Option<NaiveDateTime> {
    match schedule {
        Schedule::Hourly { interval_hours } => {
            let hours = i64::from((*interval_hours).clamp(1, 24 * 7));
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
pub(super) fn day_allowed(schedule: &Schedule, day: NaiveDate) -> bool {
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
#[must_use]
pub fn next_run_at(schedule: &Schedule, now: DateTime<Local>) -> Option<i64> {
    // An interval is that many real hours, whatever the clocks do in between.
    if let Schedule::Hourly { interval_hours } = schedule {
        let hours = i64::from((*interval_hours).clamp(1, 24 * 7));
        return Some((now + Duration::hours(hours)).timestamp_millis());
    }
    let next = next_after(schedule, now.naive_local())?;
    // A time the clocks repeat comes twice; the first of them may already be past.
    let repeated = Local.from_local_datetime(&next);
    [repeated.earliest(), repeated.latest()]
        .into_iter()
        .flatten()
        .find(|at| *at > now)
        .or_else(|| {
            Local
                .from_local_datetime(&(next + Duration::hours(1)))
                .earliest()
        })
        .map(|at| at.timestamp_millis())
}

/// `task` as it is saved: given an id if it has none, checked, and rescheduled from now, so a
/// change to the time takes effect at once.
pub(crate) fn prepare(mut task: Task) -> Result<Task, Error> {
    if task.id.trim().is_empty() {
        task.id = new_id();
    }
    if task.name.trim().is_empty() {
        return Err("A scheduled task needs a name".into());
    }
    if task.prompt.trim().is_empty() {
        return Err("A scheduled task needs a prompt".into());
    }
    // Saved as the interval it runs at, so its description says the same.
    if let Schedule::Hourly { interval_hours } = &mut task.schedule {
        *interval_hours = (*interval_hours).clamp(1, 24 * 7);
    }
    let next = next_run_at(&task.schedule, Local::now());
    if next.is_none() {
        return Err("The schedule needs a time of day, as HH:MM".into());
    }
    task.next_run_at = if task.enabled { next } else { None };
    Ok(task)
}

/// A one-line description of the schedule for a list row, e.g. `Weekdays at 09:00`.
#[must_use]
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
            if names.is_empty() {
                format!("Every day at {time}")
            } else {
                format!("{} at {time}", names.join(", "))
            }
        }
    }
}

/// How late a slot may still run: one missed while the app was closed or asleep runs when it
/// comes back, unless it went by more than this long ago. A week-old "what changed today" is
/// worth nothing.
pub const CATCH_UP_WITHIN_MS: i64 = 24 * 60 * 60 * 1000;

/// Why a skipped run was not run late, in words that follow [`CATCH_UP_WITHIN_MS`].
#[must_use]
pub fn skipped_detail() -> String {
    let hours = CATCH_UP_WITHIN_MS / (60 * 60 * 1000);
    let window = match hours {
        24 => "a day".to_string(),
        1 => "an hour".to_string(),
        h if h % 24 == 0 => format!("{} days", h / 24),
        h => format!("{h} hours"),
    };
    format!("Its time went by more than {window} ago")
}

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
#[must_use]
pub fn action(task: &Task, now_ms: i64) -> Action {
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
