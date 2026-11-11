//! How schedules work out their next run.

// Rust guideline compliant 2026-10-09

use chrono::{NaiveDateTime};

use super::times::*;
use super::types::*;

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
    let task = |enabled: bool, next_run_at: Option<i64>| Task {
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

/// The JSON the database holds, which older versions read too.
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
    let read: Schedule = serde_json::from_str(r#"{"type":"daily","time":"07:30"}"#).expect("read");
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
