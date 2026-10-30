//! Starting scheduled tasks, on time or on request.

use std::sync::Arc;

use crate::orchestration::{AgentEventEnvelope, AgentEvent};
use crate::system::now_ms;
use crate::threads::Thread;
use crate::scheduled;
use crate::threads::NewThread;
use crate::Server;

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
