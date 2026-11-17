//! The background tasks of a turn: the subagents and commands an agent leaves running, kept as the agent reports them start, change and end.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use crate::provider::TaskEvent;
use crate::provider::claude::SHELL_TASK;
use crate::events::AgentEvent;
use crate::threads::BackgroundTask;
use crate::threads::TaskStatus;

/// One task the turn has seen. An ended task stays, because the agent may report its end twice.
#[derive(Debug)]
pub(super) struct Entry {
    task: BackgroundTask,
    /// Where it was started, to list Codex's open tasks in the order they began.
    order: u64,
    /// False for a task a subagent started, which is listed under that subagent's thread instead.
    top_level: bool,
    /// Whether the agent reports the task's end, so the task holds the turn's stdin open.
    reported: bool,
    ended: bool,
    /// The message the task is listed under, once it is listed.
    shown: Option<String>,
    /// The tool call that started the task, when it returned while the task still runs: the
    /// message the call is shown in, and whether the call failed. Shown finished when the task ends.
    call: Option<(String, bool)>,
}

/// The tasks one agent process has started, and what each change to them publishes.
#[derive(Debug, Default)]
pub(in crate::orchestration) struct Background {
    /// Every task seen, by the id of the tool call that started it.
    tasks: HashMap<String, Entry>,
    /// The tool call of each task, by the id the agent reports it under.
    by_task: HashMap<String, String>,
    next_order: u64,
}

impl Background {
    /// Applies one change the agent reports for a task, and returns what to publish.
    pub(in crate::orchestration) fn apply(
        &mut self,
        event: TaskEvent,
        message_id: &str,
    ) -> Vec<AgentEvent> {
        match event {
            TaskEvent::Started {
                task,
                task_id,
                top_level,
                backgrounded,
            } => self.started(task, task_id, top_level, backgrounded, message_id),
            TaskEvent::Updated {
                task_id,
                status,
                description,
                backgrounded,
            } => self.updated(&task_id, status, description, backgrounded, message_id),
            TaskEvent::Progress { task_id, summary } => self.progress(&task_id, summary),
            TaskEvent::Ended {
                task_id,
                status,
                summary,
                output,
            } => self.ended(&task_id, status, summary, output),
            TaskEvent::Level { live } => self.level(&live),
        }
    }

    /// Records a Codex command or subagent that the agent reports no end for. It is listed by
    /// `list_unreported` if its turn completes with it still open.
    pub(in crate::orchestration) fn start_unreported(&mut self, task: BackgroundTask) {
        self.insert(task, true, false);
    }

    /// Lists the open tasks the agent reports no end for, in the order they started. Codex's
    /// commands still running when its turn completes are listed this way.
    pub(in crate::orchestration) fn list_unreported(
        &mut self,
        message_id: &str,
    ) -> Vec<AgentEvent> {
        let mut open: Vec<(u64, String)> = self
            .tasks
            .iter()
            .filter(|(_, e)| !e.reported && !e.ended)
            .map(|(id, e)| (e.order, id.clone()))
            .collect();
        open.sort_unstable();
        open.iter()
            .filter_map(|(_, tool_id)| self.list(tool_id, message_id))
            .collect()
    }

    /// Ends an unreported task whose call finished, with the call's outcome.
    pub(in crate::orchestration) fn finish_unreported(
        &mut self,
        tool_id: &str,
        failed: bool,
    ) -> Vec<AgentEvent> {
        let Some(entry) = self.tasks.get_mut(tool_id) else {
            return Vec::new();
        };
        if entry.reported || entry.ended {
            return Vec::new();
        }
        entry.ended = true;
        entry.task.status = if failed {
            TaskStatus::Failed
        } else {
            TaskStatus::Done
        };
        entry
            .shown
            .clone()
            .map(|message_id| AgentEvent::BackgroundTaskEnded {
                message_id,
                task: entry.task.clone(),
            })
            .into_iter()
            .collect()
    }

    /// Records that a tool call returned while the reported task it started still runs. Its
    /// finish is held back and shown when the task ends, so this returns true. A shell command's
    /// call is not held: it is shown finished as soon as it returns.
    pub(in crate::orchestration) fn call_returned(
        &mut self,
        tool_id: &str,
        message_id: &str,
        is_error: bool,
    ) -> bool {
        let Some(entry) = self.tasks.get_mut(tool_id) else {
            return false;
        };
        if entry.ended || !entry.reported || entry.task.kind == SHELL_TASK {
            return false;
        }
        entry.call = Some((message_id.into(), is_error));
        true
    }

    /// Whether the agent's stdin stays open. A task the agent reports holds it open until the task
    /// ends, whatever its kind, so the agent can take the report that the task ended.
    pub(in crate::orchestration) fn keeps_stdin_open(&self) -> bool {
        self.tasks.values().any(|e| e.reported && !e.ended)
    }

    /// Whether a task listed under the background is still running.
    pub(in crate::orchestration) fn any_listed_live(&self) -> bool {
        self.tasks.values().any(|e| e.shown.is_some() && !e.ended)
    }

    /// Ends every task still open, because the process that ran them has gone. A listed one is
    /// published as stopped, and a call a task still held is shown finished as failed.
    pub(in crate::orchestration) fn stop_all(&mut self) -> Vec<AgentEvent> {
        let mut open: Vec<(u64, String)> = self
            .tasks
            .iter()
            .filter(|(_, e)| !e.ended)
            .map(|(id, e)| (e.order, id.clone()))
            .collect();
        open.sort_unstable();
        open.iter()
            .flat_map(|(_, tool_id)| self.finish(tool_id, TaskStatus::Stopped))
            .collect()
    }

    /// Ends every task the agent's level leaves out. The agent sends the full set of live tasks
    /// whenever it changes, so a task it no longer names has ended, whether or not its own end
    /// was reported; without this, a lost end leaves the task listed as running for good. A task
    /// the level names before the agent has reported it starting is left for that report, which
    /// is what lists it.
    fn level(&mut self, live: &[String]) -> Vec<AgentEvent> {
        let gone: Vec<String> = self
            .tasks
            .iter()
            .filter(|(_, e)| !e.ended)
            .filter(|(_, e)| e.task.task_id.as_ref().is_some_and(|id| !live.contains(id)))
            .map(|(tool_id, _)| tool_id.clone())
            .collect();
        gone.iter()
            .flat_map(|tool_id| self.finish(tool_id, TaskStatus::Stopped))
            .collect()
    }

    /// Ends one open task as `status`, giving up the call it held. Returns what to publish: the
    /// end of its listing, then its held call shown finished.
    fn finish(&mut self, tool_id: &str, status: TaskStatus) -> Vec<AgentEvent> {
        let Some(entry) = self.tasks.get_mut(tool_id) else {
            return Vec::new();
        };
        if entry.ended {
            return Vec::new();
        }
        entry.ended = true;
        entry.task.status = status;
        let call = entry.call.take();
        let mut events = Vec::new();
        if let Some(message_id) = entry.shown.clone() {
            events.push(AgentEvent::BackgroundTaskEnded {
                message_id,
                task: entry.task.clone(),
            });
        }
        if let Some((message_id, call_failed)) = call {
            let failed = status != TaskStatus::Done || call_failed;
            events.push(finished_call(message_id, tool_id.into(), failed));
        }
        events
    }

    fn started(
        &mut self,
        task: BackgroundTask,
        task_id: String,
        top_level: bool,
        backgrounded: bool,
        message_id: &str,
    ) -> Vec<AgentEvent> {
        let tool_id = task.tool_id.clone();
        self.by_task.insert(task_id, tool_id.clone());
        self.insert(task, top_level, true);
        if backgrounded {
            self.list(&tool_id, message_id).into_iter().collect()
        } else {
            Vec::new()
        }
    }

    fn updated(
        &mut self,
        task_id: &str,
        status: Option<TaskStatus>,
        description: Option<String>,
        backgrounded: bool,
        message_id: &str,
    ) -> Vec<AgentEvent> {
        let Some(tool_id) = self.tool_of(task_id) else {
            return Vec::new();
        };
        let Some(entry) = self.tasks.get_mut(&tool_id) else {
            return Vec::new();
        };
        if entry.ended {
            return Vec::new();
        }
        if let Some(status) = status {
            entry.task.status = status;
        }
        if let Some(description) = description {
            entry.task.description = description;
        }
        let listed = if backgrounded {
            self.list(&tool_id, message_id)
        } else {
            None
        };
        listed
            .or_else(|| self.refresh(&tool_id))
            .into_iter()
            .collect()
    }

    fn progress(&mut self, task_id: &str, summary: String) -> Vec<AgentEvent> {
        let Some(tool_id) = self.tool_of(task_id) else {
            return Vec::new();
        };
        let Some(entry) = self.tasks.get_mut(&tool_id) else {
            return Vec::new();
        };
        if entry.ended {
            return Vec::new();
        }
        entry.task.summary = Some(summary);
        self.refresh(&tool_id).into_iter().collect()
    }

    /// The first report of an end ends the task, and frees the call it held. Each later report
    /// (the status, or the notification with the output, whichever comes second) only fills in
    /// what it has, and republishes the task.
    fn ended(
        &mut self,
        task_id: &str,
        status: TaskStatus,
        summary: Option<String>,
        output: Option<String>,
    ) -> Vec<AgentEvent> {
        let Some(tool_id) = self.tool_of(task_id) else {
            return Vec::new();
        };
        let Some(entry) = self.tasks.get_mut(&tool_id) else {
            return Vec::new();
        };
        let first = !entry.ended;
        entry.ended = true;
        entry.task.status = status;
        if summary.is_some() {
            entry.task.summary = summary;
        }
        if output.is_some() {
            entry.task.output = output;
        }
        let call = entry.call.take();
        let shown = entry.shown.clone();
        let task = entry.task.clone();

        let mut events = Vec::new();
        if let Some(message_id) = shown {
            events.push(if first {
                AgentEvent::BackgroundTaskEnded { message_id, task }
            } else {
                AgentEvent::BackgroundTaskUpdated { message_id, task }
            });
        }
        if let Some((message_id, call_failed)) = call {
            let failed = status != TaskStatus::Done || call_failed;
            events.push(finished_call(message_id, tool_id, failed));
        }
        events
    }

    fn insert(&mut self, task: BackgroundTask, top_level: bool, reported: bool) {
        self.next_order += 1;
        let entry = Entry {
            task,
            order: self.next_order,
            top_level,
            reported,
            ended: false,
            shown: None,
            call: None,
        };
        self.tasks.insert(entry.task.tool_id.clone(), entry);
    }

    fn tool_of(&self, task_id: &str) -> Option<String> {
        self.by_task.get(task_id).cloned()
    }

    /// Lists an open task that is top-level under `message_id`. The listing is shown once.
    fn list(&mut self, tool_id: &str, message_id: &str) -> Option<AgentEvent> {
        let entry = self.tasks.get_mut(tool_id)?;
        if !entry.top_level || entry.ended || entry.shown.is_some() {
            return None;
        }
        entry.shown = Some(message_id.into());
        Some(AgentEvent::BackgroundTaskStarted {
            message_id: message_id.into(),
            task: entry.task.clone(),
        })
    }

    /// The latest record of a listed task, for the message it is shown in.
    fn refresh(&self, tool_id: &str) -> Option<AgentEvent> {
        let entry = self.tasks.get(tool_id)?;
        let message_id = entry.shown.clone()?;
        Some(AgentEvent::BackgroundTaskUpdated {
            message_id,
            task: entry.task.clone(),
        })
    }
}

/// A held tool call, shown finished.
pub(super) fn finished_call(message_id: String, tool_id: String, is_error: bool) -> AgentEvent {
    AgentEvent::ToolFinished {
        message_id,
        tool_id,
        is_error,
        output: None,
    }
}
