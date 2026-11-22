//! Saving scheduled tasks and running the ones that are due.

// Rust guideline compliant 2026-10-09

use super::times::prepare;
use super::types::Task;
use crate::Error;
use std::sync::Arc;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::scheduled;
use crate::system::now_ms;
use crate::chats::{NewChat, Chat};
use crate::Core;

impl Core {
    /// Starts a task in a new chat for `task`, and returns the chat.
    ///
    /// A task whose project no longer exists runs as a chat, so a deleted project does not stop
    /// the schedule without saying so.
    pub(crate) fn start_scheduled_task(
        self: &Arc<Self>,
        task: &crate::scheduled::Task,
    ) -> Result<Chat, Error> {
        let chat = match &task.agent_id {
            Some(agent) => self.start_agent_routine(agent, task)?,
            None => self.start_plain_task(task)?,
        };
        // Named after the task, not after its prompt: the first message is what the schedule says,
        // and the task's own name is what the list should show. This comes after the message so
        // the naming a new chat gets from its first line cannot take the name back.
        self.store
            .update_chat(&chat.id, |t| t.title.clone_from(&task.name))?;
        self.emit(AgentEventEnvelope {
            chat_id: chat.id.clone(),
            event: AgentEvent::TitleUpdated {
                title: task.name.clone(),
            },
        });
        let chat = self.store.chat(&chat.id).unwrap_or(chat);
        Ok(chat)
    }

    /// Runs agent `agent_id` with the task's prompt. A project the agent is no longer linked to is
    /// dropped, so the run happens in the agent's own folder.
    fn start_agent_routine(self: &Arc<Self>, agent_id: &str, task: &Task) -> Result<Chat, Error> {
        let linked = self.agent(agent_id).map(|b| b.projects).unwrap_or_default();
        let project_id = task.project_id.clone().filter(|id| linked.contains(id));
        self.run_agent(agent_id, &task.prompt, project_id)
    }

    /// Starts the task's prompt on its own agent in a new chat.
    fn start_plain_task(self: &Arc<Self>, task: &Task) -> Result<Chat, Error> {
        let project_id = task.project_id.clone().filter(|id| {
            self.store
                .settings()
                .projects
                .iter()
                .any(|project| &project.id == id)
        });
        let chat = self.create_chat(NewChat {
            id: None,
            project_id,
            agent: task.agent,
            model: task.model.clone(),
            access_mode: task.access_mode,
            effort: None,
            compact_window: None,
        })?;
        let chat = self.store.chat(&chat.id).unwrap_or(chat);
        self.emit(AgentEventEnvelope {
            chat_id: chat.id.clone(),
            event: AgentEvent::ChatCreated { chat: chat.clone() },
        });
        // A task that cannot start leaves no empty chat behind; the task records why it failed.
        if let Err(error) =
            self.send_message(chat.id.clone(), task.prompt.clone(), Vec::new(), false)
        {
            self.delete_chat(&chat.id);
            self.emit(AgentEventEnvelope {
                chat_id: chat.id,
                event: AgentEvent::ChatDeleted,
            });
            return Err(error);
        }
        Ok(chat)
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

    /// Every scheduled task.
    #[must_use]
    pub fn scheduled_tasks(&self) -> Vec<Task> {
        self.store.scheduled_tasks()
    }

    /// Deletes the scheduled task `id` and its runs.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot delete the task or its runs.
    pub fn remove_scheduled_task(&self, id: &str) -> Result<(), Error> {
        self.store.remove_scheduled_task(id)
    }

    /// Starts one scheduled task now, whatever its schedule says.
    ///
    /// # Errors
    ///
    /// Returns an error when the scheduled task no longer exists, its chat cannot be created or
    /// started, or recording the run fails.
    pub fn run_scheduled_task(self: &Arc<Self>, id: &str) -> Result<Chat, Error> {
        let task = self
            .store
            .scheduled_tasks()
            .into_iter()
            .find(|task| task.id == id)
            .ok_or("The scheduled task no longer exists")?;
        let chat = self.start_scheduled_task(&task)?;
        let now = now_ms() as i64;
        let next = scheduled::next_run_at(&task.schedule, chrono::Local::now());
        self.store
            .scheduled_task_ran(&task.id, now, next, Ok(chat.id.as_str()))?;
        Ok(chat)
    }

    /// Whether a reply is under way in a chat of `project_id`. Chats have no project and never
    /// collide, since each has a scratch folder of its own.
    fn busy_in_project(&self, project_id: Option<&str>) -> bool {
        let Some(project_id) = project_id else {
            return false;
        };
        self.runner.running().iter().any(|chat_id| {
            self.store
                .chat(chat_id)
                .is_some_and(|chat| chat.project_id.as_deref() == Some(project_id))
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
                    if let Err(e) = self.store.scheduled_task_skipped(
                        &task.id,
                        next(),
                        super::times::skipped_detail(),
                    ) {
                        log::warn!(
                            task:% = task.id, error:% = e;
                            "scheduled task {{task}}: {{error}}"
                        );
                    }
                }
                // It stays due, so the next tick tries again.
                scheduled::Action::Run if self.busy_in_project(task.project_id.as_deref()) => {}
                scheduled::Action::Run => match self.start_scheduled_task(task) {
                    Ok(chat) => {
                        let ran = self.store.scheduled_task_ran(
                            &task.id,
                            now,
                            next(),
                            Ok(chat.id.as_str()),
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
                        let ran = self.store.scheduled_task_ran(
                            &task.id,
                            now,
                            next(),
                            Err(e.to_string().as_str()),
                        );
                        if let Err(e) = ran {
                            log::warn!(
                                task:% = task.id, error:% = e;
                                "scheduled task {{task}}: {{error}}"
                            );
                        }
                    }
                },
            }
        }
        started
    }
}
