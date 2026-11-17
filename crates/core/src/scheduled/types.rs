//! A scheduled task, a run of it, and when it is set to run.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};
use crate::provider::AgentKind;
use crate::threads::AccessMode;

/// A scheduled task: a prompt that starts a task at a fixed time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
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
    pub recent_runs: Vec<Run>,
}

/// One time a scheduled task's slot came round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    /// When it started, or for a skipped run the slot it missed, in Unix milliseconds.
    pub at: i64,
    pub thread_id: Option<String>,
    pub outcome: RunOutcome,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunOutcome {
    Started,
    Failed,
    /// Its time went by longer ago than `CATCH_UP_WITHIN_MS`, so it was not run late.
    Skipped,
}

/// How often a scheduled task runs. The shapes Codex's own tasks use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    #[must_use]
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
    #[must_use]
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
