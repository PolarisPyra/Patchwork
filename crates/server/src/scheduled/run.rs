//! Saving scheduled tasks and running the ones that are due.

// Rust guideline compliant 2026-10-09

use super::times::prepare;
use super::types::Task;
use crate::Error;
use std::sync::Arc;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::scheduled;
use crate::system::now_ms;
use crate::threads::{NewThread, Thread};
use crate::Server;

impl Server {
    /// Starts a task in a new thread for `task`, and returns the thread.
    ///
    /// A task whose project no longer exists runs as a chat, so a deleted project does not stop
    /// the schedule without saying so.
    pub(crate) fn start_scheduled_task(
        self: &Arc<Self>,
        task: &crate::scheduled::Task,
    ) -> Result<Thread, Error> {
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
        // A task that cannot start leaves no empty chat behind; the task records why it failed.
        if let Err(error) =
            self.send_message(thread.id.clone(), task.prompt.clone(), Vec::new(), false)
        {
            self.delete_thread(&thread.id);
            self.emit(AgentEventEnvelope {
                thread_id: thread.id,
                event: AgentEvent::ThreadDeleted,
            });
            return Err(error);
        }
        // Named after the task, not after its prompt: the first message is what the schedule says,
        // and the task's own name is what the list should show. This comes after the message so
        // the naming a new thread gets from its first line cannot take the name back.
        self.store
            .update_thread(&thread.id, |t| t.title.clone_from(&task.name))?;
        self.emit(AgentEventEnvelope {
            thread_id: thread.id.clone(),
            event: AgentEvent::TitleUpdated {
                title: task.name.clone(),
            },
        });
        let thread = self.store.thread(&thread.id).unwrap_or(thread);
        Ok(thread)
    }

    /// Adds a task or replaces the one with its id, and returns it as saved. See `prepare`.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has no name or prompt, its schedule has no valid `HH:MM`
    /// time, or saving it fails.
    pub fn save_scheduled_task(&self, task: Task) -> Result<Task, Error> {
        let task = prepare(task)?;
        self.store.save_scheduled_task(&task)?;
        Ok(task)
    }

    /// Starts one scheduled task now, whatever its schedule says.
    ///
    /// # Errors
    ///
    /// Returns an error when the scheduled task no longer exists, its thread cannot be created or
    /// started, or recording the run fails.
    pub fn run_scheduled_task(self: &Arc<Self>, id: &str) -> Result<Thread, Error> {
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
                    log::info!(
                        task:% = task.name;
                        "scheduled task {{task}} missed its time; skipped"
                    );
                    if let Err(e) = self.store.scheduled_task_skipped(&task.id, next()) {
                        log::warn!(
                            task:% = task.id, error:% = e;
                            "scheduled task {{task}}: {{error}}"
                        );
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
                            log::warn!(
                                task:% = task.id, error:% = e;
                                "scheduled task {{task}}: {{error}}"
                            );
                        }
                        started.push(task.name.clone());
                    }
                    Err(e) => {
                        log::warn!(
                            task:% = task.name, error:% = e;
                            "scheduled task {{task}} did not start: {{error}}"
                        );
                        // Record the attempt so a task that cannot start does not retry every tick.
                        let _ = self.store.scheduled_task_ran(
                            &task.id,
                            now,
                            next(),
                            Err(e.to_string().as_str()),
                        );
                    }
                },
            }
        }
        started
    }
}
