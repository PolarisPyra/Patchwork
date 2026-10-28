//! The `scheduled_tasks` table: a prompt, its schedule, and when it last and next runs; and
//! `scheduled_task_runs`, the newest few runs of each.

use std::collections::HashMap;

use rusqlite::{OptionalExtension, params};

use super::codec::{dec, enc, err, json};
use super::store::{Inner, Store};
use crate::types::{
    AccessMode, AgentKind, RunOutcome, Schedule, ScheduledRun, ScheduledTask, new_id, now_ms,
};

/// How many runs each task keeps.
const RECENT_RUNS: i64 = 5;

struct Row {
    id: String,
    name: String,
    prompt: String,
    schedule: String,
    project_id: Option<String>,
    agent: AgentKind,
    model: Option<String>,
    access_mode: AccessMode,
    enabled: bool,
    last_run_at: Option<i64>,
    next_run_at: Option<i64>,
    created_at: i64,
}

pub(super) fn load(inner: &Inner) -> crate::Res<Vec<ScheduledTask>> {
    let mut stmt = inner
        .conn
        .prepare(
            "SELECT id, name, prompt, schedule, project_id, agent, model, access_mode, enabled,
                    last_run_at, next_run_at, created_at
             FROM scheduled_tasks ORDER BY created_at",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Row {
                id: row.get(0)?,
                name: row.get(1)?,
                prompt: row.get(2)?,
                schedule: row.get(3)?,
                project_id: row.get(4)?,
                agent: dec(row.get::<_, String>(5)?)?,
                model: row.get(6)?,
                access_mode: dec(row.get::<_, String>(7)?)?,
                enabled: row.get::<_, i64>(8)? != 0,
                last_run_at: row.get(9)?,
                next_run_at: row.get(10)?,
                created_at: row.get(11)?,
            })
        })
        .map_err(err)?;
    let rows = rows.collect::<Result<Vec<_>, _>>().map_err(err)?;
    let mut runs = recent_runs(inner)?;
    rows.into_iter()
        .map(|row| {
            let schedule: Schedule = serde_json::from_str(&row.schedule)
                .map_err(|e| format!("Database error: scheduled task {}: {e}", row.id))?;
            let recent_runs = runs.remove(&row.id).unwrap_or_default();
            Ok(ScheduledTask {
                id: row.id,
                name: row.name,
                prompt: row.prompt,
                schedule,
                project_id: row.project_id,
                agent: row.agent,
                model: row.model,
                access_mode: row.access_mode,
                enabled: row.enabled,
                last_run_at: row.last_run_at,
                next_run_at: row.next_run_at,
                created_at: row.created_at,
                recent_runs,
            })
        })
        .collect()
}

/// The newest runs of every task, newest first, by task id, in one query.
fn recent_runs(inner: &Inner) -> crate::Res<HashMap<String, Vec<ScheduledRun>>> {
    let mut stmt = inner
        .conn
        .prepare(
            "SELECT task_id, started_at, thread_id, outcome, detail FROM (
                 SELECT *, ROW_NUMBER() OVER (
                     PARTITION BY task_id ORDER BY started_at DESC, id DESC
                 ) AS n
                 FROM scheduled_task_runs
             )
             WHERE n <= ?1
             ORDER BY task_id, started_at DESC, id DESC",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map([RECENT_RUNS], |row| {
            Ok((
                row.get::<_, String>(0)?,
                ScheduledRun {
                    at: row.get(1)?,
                    thread_id: row.get(2)?,
                    outcome: dec(row.get::<_, String>(3)?)?,
                    detail: row.get(4)?,
                },
            ))
        })
        .map_err(err)?;
    let mut runs: HashMap<String, Vec<ScheduledRun>> = HashMap::new();
    for row in rows {
        let (task_id, run) = row.map_err(err)?;
        runs.entry(task_id).or_default().push(run);
    }
    Ok(runs)
}

/// Adds a run to a task's history and drops all but its newest few.
fn record_run(inner: &Inner, task_id: &str, run: &ScheduledRun) -> crate::Res<()> {
    inner
        .conn
        .execute(
            "INSERT INTO scheduled_task_runs (task_id, started_at, thread_id, outcome, detail)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![task_id, run.at, run.thread_id, enc(&run.outcome), run.detail],
        )
        .map_err(err)?;
    inner
        .conn
        .execute(
            "DELETE FROM scheduled_task_runs WHERE task_id = ?1 AND id NOT IN (
                 SELECT id FROM scheduled_task_runs WHERE task_id = ?1
                 ORDER BY started_at DESC, id DESC LIMIT ?2
             )",
            params![task_id, RECENT_RUNS],
        )
        .map_err(err)?;
    Ok(())
}

fn save(inner: &Inner, task: &ScheduledTask) -> crate::Res<()> {
    inner
        .conn
        .execute(
            "INSERT INTO scheduled_tasks
                 (id, name, prompt, schedule, project_id, agent, model, access_mode, enabled,
                  last_run_at, next_run_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, prompt = excluded.prompt,
                 schedule = excluded.schedule, project_id = excluded.project_id,
                 agent = excluded.agent, model = excluded.model,
                 access_mode = excluded.access_mode, enabled = excluded.enabled,
                 last_run_at = excluded.last_run_at, next_run_at = excluded.next_run_at",
            params![
                task.id,
                task.name,
                task.prompt,
                json(&task.schedule),
                task.project_id,
                enc(&task.agent),
                task.model,
                enc(&task.access_mode),
                i64::from(task.enabled),
                task.last_run_at,
                task.next_run_at,
                task.created_at,
            ],
        )
        .map_err(err)?;
    Ok(())
}

impl Store {
    pub fn scheduled_tasks(&self) -> Vec<ScheduledTask> {
        let inner = self.lock();
        match load(&inner) {
            Ok(tasks) => tasks,
            Err(e) => {
                // A row that cannot be read would otherwise stop every schedule without a word,
                // and the timer would look like it was working.
                log::error!("scheduled tasks could not be read: {e}");
                Vec::new()
            }
        }
    }

    /// Adds a task, or replaces the one with its id. A task without an id is given one.
    pub fn save_scheduled_task(&self, mut task: ScheduledTask) -> crate::Res<ScheduledTask> {
        if task.id.trim().is_empty() {
            task.id = new_id();
        }
        if task.name.trim().is_empty() {
            return Err("A scheduled task needs a name".into());
        }
        if task.prompt.trim().is_empty() {
            return Err("A scheduled task needs a prompt".into());
        }
        let next = crate::scheduled::next_run_at(&task.schedule, chrono::Local::now());
        if next.is_none() {
            return Err("The schedule needs a time of day, as HH:MM".into());
        }
        // Saving a task reschedules it from now, so a change to the time takes effect at once.
        task.next_run_at = if task.enabled { next } else { None };
        let inner = self.lock();
        save(&inner, &task)?;
        Ok(task)
    }

    pub fn remove_scheduled_task(&self, id: &str) -> crate::Res<()> {
        let inner = self.lock();
        inner
            .conn
            .execute("DELETE FROM scheduled_tasks WHERE id = ?1", [id])
            .map_err(err)?;
        inner
            .conn
            .execute("DELETE FROM scheduled_task_runs WHERE task_id = ?1", [id])
            .map_err(err)?;
        Ok(())
    }

    /// Records that a task was started at `at`, and will next start at `next`. `started` is the
    /// thread it started, or why it did not start one.
    pub fn scheduled_task_ran(
        &self,
        id: &str,
        at: i64,
        next: Option<i64>,
        started: Result<&str, &str>,
    ) -> crate::Res<()> {
        let inner = self.lock();
        let changed = inner
            .conn
            .execute(
                "UPDATE scheduled_tasks SET last_run_at = ?2, next_run_at = ?3 WHERE id = ?1",
                params![id, at, next],
            )
            .map_err(err)?;
        if changed == 0 {
            return Err("The scheduled task no longer exists".into());
        }
        let run = match started {
            Ok(thread_id) => ScheduledRun {
                at,
                thread_id: Some(thread_id.to_string()),
                outcome: RunOutcome::Started,
                detail: None,
            },
            Err(why) => ScheduledRun {
                at,
                thread_id: None,
                outcome: RunOutcome::Failed,
                detail: Some(why.to_string()),
            },
        };
        record_run(&inner, id, &run)
    }

    /// Records that a task's slot went by too long ago to run late, and moves it on to `next`.
    /// The skipped run is dated by the slot it missed.
    pub fn scheduled_task_skipped(&self, id: &str, next: Option<i64>) -> crate::Res<()> {
        let inner = self.lock();
        let missed: Option<i64> = inner
            .conn
            .query_row(
                "SELECT next_run_at FROM scheduled_tasks WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(err)?
            .ok_or("The scheduled task no longer exists")?;
        inner
            .conn
            .execute(
                "UPDATE scheduled_tasks SET next_run_at = ?2 WHERE id = ?1",
                params![id, next],
            )
            .map_err(err)?;
        let run = ScheduledRun {
            at: missed.unwrap_or_else(|| now_ms() as i64),
            thread_id: None,
            outcome: RunOutcome::Skipped,
            detail: Some("Its time went by more than a day ago".into()),
        };
        record_run(&inner, id, &run)
    }

    /// Whether the table has a task with this id.
    pub fn has_scheduled_task(&self, id: &str) -> bool {
        let inner = self.lock();
        inner
            .conn
            .query_row("SELECT 1 FROM scheduled_tasks WHERE id = ?1", [id], |_| {
                Ok(())
            })
            .optional()
            .ok()
            .flatten()
            .is_some()
    }
}
