//! Scheduled tasks: a prompt that starts a task on a schedule, the way Codex and Claude desktop
//! run theirs. The schedule is one of a fixed few shapes, so its next run can be worked out from
//! the local clock without a cron parser.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

use crate::types::{Schedule, ScheduledTask, Weekday};

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

/// The first run after `now`, as Unix milliseconds.
pub fn next_run_at(schedule: &Schedule, now: DateTime<Local>) -> Option<i64> {
    let next = next_after(schedule, now.naive_local())?;
    Local
        .from_local_datetime(&next)
        .earliest()
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

/// Tasks that are due now: enabled, and with a next run at or before `now_ms`. A task with no
/// next run yet gets one, so a newly saved task does not fire immediately.
pub fn due(tasks: &[ScheduledTask], now_ms: i64) -> Vec<&ScheduledTask> {
    tasks
        .iter()
        .filter(|task| task.enabled && task.next_run_at.is_some_and(|at| at <= now_ms))
        .collect()
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
    fn due_only_lists_enabled_tasks_whose_time_has_come() {
        let task = |enabled: bool, next_run_at: Option<i64>| ScheduledTask {
            id: "t".into(),
            name: "n".into(),
            prompt: "p".into(),
            schedule: Schedule::Daily {
                time: "09:00".into(),
            },
            project_id: None,
            agent: crate::types::AgentKind::Claude,
            model: None,
            access_mode: crate::types::AccessMode::Supervised,
            enabled,
            last_run_at: None,
            next_run_at,
            created_at: 0,
        };
        let tasks = vec![
            task(true, Some(10)),
            task(false, Some(10)),
            task(true, None),
        ];
        let now = 10;
        let names: Vec<&str> = due(&tasks, now)
            .iter()
            .map(|task| task.name.as_str())
            .collect();
        assert_eq!(names, vec!["n"]);
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
