//! The `scheduled_tasks` table: a prompt, its schedule, and when it last and next runs.

use rusqlite::{OptionalExtension, params};

use super::codec::{dec, enc, err, json};
use super::store::{Inner, Store};
use crate::types::{AccessMode, AgentKind, Schedule, ScheduledTask, new_id};

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
    rows.into_iter()
        .map(|row| {
            let schedule: Schedule = serde_json::from_str(&row.schedule)
                .map_err(|e| format!("Database error: scheduled task {}: {e}", row.id))?;
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
            })
        })
        .collect()
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
        Ok(())
    }

    /// Records that a task started at `at` and will next start at `next`.
    pub fn scheduled_task_ran(&self, id: &str, at: i64, next: Option<i64>) -> crate::Res<()> {
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
        Ok(())
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
