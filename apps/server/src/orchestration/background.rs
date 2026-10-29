//! The tasks an agent started and left running: what each one is, whether it holds the turn's stdin
//! open, and which of them are listed under the message they started in.
//!
//! Claude reports each task in the stream, and the CLI reports its end by itself, so a task it
//! reports holds stdin open until it ends. Claude also sends the full set of tasks still running
//! whenever it changes, which ends a task whose own end never reached us. Codex reports a command
//! or subagent only through the call that started it. One still open when its turn completes is
//! listed, but Codex never reports it ending, so it holds nothing and the turn ends with it.
use std::collections::HashMap;

use crate::provider::TaskEvent;
use crate::provider::claude::SHELL_TASK;
use crate::types::AgentEvent;
use crate::types::BackgroundTask;
use crate::types::TaskStatus;

/// One task the turn has seen. An ended task stays, because the agent may report its end twice.
#[derive(Debug)]
struct Entry {
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
pub(super) struct Background {
    /// Every task seen, by the id of the tool call that started it.
    tasks: HashMap<String, Entry>,
    /// The tool call of each task, by the id the agent reports it under.
    by_task: HashMap<String, String>,
    next_order: u64,
}

impl Background {
    /// Applies one change the agent reports for a task, and returns what to publish.
    pub(super) fn apply(&mut self, event: TaskEvent, message_id: &str) -> Vec<AgentEvent> {
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
    pub(super) fn start_unreported(&mut self, task: BackgroundTask) {
        self.insert(task, true, false);
    }

    /// Lists the open tasks the agent reports no end for, in the order they started. Codex's
    /// commands still running when its turn completes are listed this way.
    pub(super) fn list_unreported(&mut self, message_id: &str) -> Vec<AgentEvent> {
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
    pub(super) fn finish_unreported(&mut self, tool_id: &str, failed: bool) -> Vec<AgentEvent> {
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
    pub(super) fn call_returned(
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
    pub(super) fn keeps_stdin_open(&self) -> bool {
        self.tasks.values().any(|e| e.reported && !e.ended)
    }

    /// Whether a task listed under the background is still running.
    pub(super) fn any_listed_live(&self) -> bool {
        self.tasks.values().any(|e| e.shown.is_some() && !e.ended)
    }

    /// Ends every task still open, because the process that ran them has gone. A listed one is
    /// published as stopped, and a call a task still held is shown finished as failed.
    pub(super) fn stop_all(&mut self) -> Vec<AgentEvent> {
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
fn finished_call(message_id: String, tool_id: String, is_error: bool) -> AgentEvent {
    AgentEvent::ToolFinished {
        message_id,
        tool_id,
        is_error,
        output: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Parsed;
    use crate::provider::TurnSpec;
    use crate::provider::claude;
    use crate::provider::codex;
    use crate::provider::codex::fixtures as codex_fixtures;
    use crate::types::AccessMode;
    use crate::provider::claude::fixtures::AGENT_LAUNCHED;
    use crate::provider::claude::fixtures::AGENT_NOTIFIED;
    use crate::provider::claude::fixtures::AGENT_STARTED;
    use crate::provider::claude::fixtures::AGENT_UPDATED;
    use crate::provider::claude::fixtures::BG_SHELL_LEVEL;
    use crate::provider::claude::fixtures::BG_SHELL_LEVEL_EMPTY;
    use crate::provider::claude::fixtures::BG_SHELL_LEVEL_STARTED;
    use crate::provider::claude::fixtures::BG_SHELL_NOTIFIED;
    use crate::provider::claude::fixtures::BG_SHELL_STARTED;
    use crate::provider::claude::fixtures::BG_SHELL_UPDATED;
    use crate::provider::claude::fixtures::OWNED_SHELL_NOTIFIED;
    use crate::provider::claude::fixtures::OWNED_SHELL_STARTED;
    use crate::provider::claude::fixtures::PROMOTED_ENDED;
    use crate::provider::claude::fixtures::PROMOTED_RESULT;
    use crate::provider::claude::fixtures::PROMOTED_STARTED;
    use crate::provider::claude::fixtures::PROMOTED_UPDATED;
    use crate::provider::claude::fixtures::TURN_RESULT;

    /// Replays captured Claude lines the way the reader handles them: each task change goes to
    /// `Background`, and each finished tool call goes through the same hold. Returns, for each
    /// line, what it published and whether stdin stays open afterwards.
    fn replay(background: &mut Background, lines: &[&str]) -> Vec<(Vec<AgentEvent>, bool)> {
        lines
            .iter()
            .map(|line| {
                let value = serde_json::from_str(line).unwrap();
                let mut events = Vec::new();
                for item in claude::parse(&value, "/abs", "m1") {
                    match item {
                        Parsed::Task(event) => events.extend(background.apply(event, "m1")),
                        Parsed::Event(AgentEvent::ToolFinished {
                            tool_id, is_error, ..
                        }) => {
                            background.call_returned(&tool_id, "m1", is_error);
                        }
                        _ => {}
                    }
                }
                (events, background.keeps_stdin_open())
            })
            .collect()
    }

    #[test]
    fn a_turn_ending_while_a_subagent_runs_keeps_stdin_open_until_it_ends() {
        // The subagent's launch returns at once and its own command ends. The turn's reply ends
        // while the subagent still runs, and stdin must stay open for its report.
        let mut background = Background::default();
        let open = replay(
            &mut background,
            &[
                AGENT_STARTED,
                AGENT_LAUNCHED,
                OWNED_SHELL_STARTED,
                TURN_RESULT,
                OWNED_SHELL_NOTIFIED,
                AGENT_UPDATED,
                AGENT_NOTIFIED,
            ],
        );
        let open: Vec<bool> = open.into_iter().map(|(_, stdin)| stdin).collect();
        assert_eq!(open, [true, true, true, true, true, false, false]);
    }

    #[test]
    fn a_status_and_a_notification_both_end_a_task_in_either_order() {
        // The status comes first, as it does in a live stream; the output arrives with the notification.
        let mut background = Background::default();
        let events = replay(
            &mut background,
            &[BG_SHELL_STARTED, BG_SHELL_UPDATED, BG_SHELL_NOTIFIED],
        );
        let [(started, _), (ended, _), (notified, _)] = events.as_slice() else {
            panic!("expected three replies");
        };
        assert!(matches!(
            started.as_slice(),
            [AgentEvent::BackgroundTaskStarted { .. }]
        ));
        assert!(matches!(
            ended.as_slice(),
            [AgentEvent::BackgroundTaskEnded { task, .. }] if task.output.is_none()
        ));
        // The notification keeps the status it has and adds the output, republishing the task.
        assert!(matches!(
            notified.as_slice(),
            [AgentEvent::BackgroundTaskUpdated { task, .. }]
                if task.status == TaskStatus::Done && task.output.is_some()
        ));
    }

    #[test]
    fn a_notification_before_the_status_keeps_its_output() {
        let mut background = Background::default();
        let events = replay(
            &mut background,
            &[BG_SHELL_STARTED, BG_SHELL_NOTIFIED, BG_SHELL_UPDATED],
        );
        let [(_, _), (notified, _), (ended, _)] = events.as_slice() else {
            panic!("expected three replies");
        };
        assert!(matches!(
            notified.as_slice(),
            [AgentEvent::BackgroundTaskEnded { task, .. }] if task.output.is_some()
        ));
        // The status that comes second does not lose the output the notification carried.
        assert!(matches!(
            ended.as_slice(),
            [AgentEvent::BackgroundTaskUpdated { task, .. }] if task.output.is_some()
        ));
    }

    #[test]
    fn a_promoted_command_is_listed_when_the_agent_moves_it_to_the_background() {
        let mut background = Background::default();
        let events = replay(
            &mut background,
            &[
                PROMOTED_STARTED,
                PROMOTED_UPDATED,
                PROMOTED_RESULT,
                PROMOTED_ENDED,
            ],
        );
        // Started in the foreground, it is not listed until the agent moves it.
        assert!(events[0].0.is_empty());
        assert!(matches!(
            events[1].0.as_slice(),
            [AgentEvent::BackgroundTaskStarted { task, .. }]
                if task.task_id.as_deref() == Some("ba28p82m4") && task.kind == "local_bash"
        ));
        assert!(matches!(
            events[3].0.as_slice(),
            [AgentEvent::BackgroundTaskEnded { task, .. }] if task.status == TaskStatus::Done
        ));
    }

    #[test]
    fn a_held_call_is_shown_finished_when_its_task_ends() {
        let mut background = Background::default();
        let events = replay(&mut background, &[AGENT_STARTED, AGENT_LAUNCHED]);
        assert!(
            events[0]
                .0
                .iter()
                .any(|e| matches!(e, AgentEvent::BackgroundTaskStarted { .. }))
        );
        // The launch's result was held: no finish was published for the call yet.
        assert!(events[1].0.is_empty());

        // The task's end finishes the call, in the same publish as the task's own end.
        let ended = replay(&mut background, &[AGENT_UPDATED, AGENT_NOTIFIED]);
        assert!(matches!(
            ended[0].0.as_slice(),
            [
                AgentEvent::BackgroundTaskEnded { .. },
                AgentEvent::ToolFinished { tool_id, is_error: false, .. },
            ] if tool_id == "toolu_01GQWb3a5rvJrVhxJtrPz2BN"
        ));
    }

    #[test]
    fn a_task_the_level_drops_ends_even_when_its_own_end_never_arrived() {
        // The captured level, its start line, and the level that drops it. The task's own end
        // (its status patch and its notification) is left out, as it is when one is lost: the
        // level is the only report that still says the task is over.
        let mut background = Background::default();
        let lines = replay(
            &mut background,
            &[BG_SHELL_LEVEL, BG_SHELL_LEVEL_STARTED, BG_SHELL_LEVEL_EMPTY],
        );
        // The first level names the task before its start line, so it lists nothing.
        assert!(lines[0].0.is_empty());
        assert!(matches!(
            lines[1].0.as_slice(),
            [AgentEvent::BackgroundTaskStarted { task, .. }]
                if task.task_id.as_deref() == Some("bad1lsfze")
        ));
        assert!(matches!(
            lines[2].0.as_slice(),
            [AgentEvent::BackgroundTaskEnded { task, .. }] if task.status == TaskStatus::Stopped
        ));
        // The task no longer holds the turn's stdin open, so the turn can end.
        assert!(!lines[2].1);
        assert!(!background.any_listed_live());
    }

    #[test]
    fn a_level_that_still_names_the_task_leaves_it_running() {
        let mut background = Background::default();
        let events = replay(&mut background, &[BG_SHELL_LEVEL_STARTED, BG_SHELL_LEVEL]);
        assert!(matches!(
            events[0].0.as_slice(),
            [AgentEvent::BackgroundTaskStarted { .. }]
        ));
        assert!(events[1].0.is_empty());
        assert!(background.keeps_stdin_open() && background.any_listed_live());
    }

    #[test]
    fn a_process_that_ends_stops_its_open_tasks_and_their_calls() {
        let mut background = Background::default();
        replay(&mut background, &[AGENT_STARTED, AGENT_LAUNCHED]);
        let events = background.stop_all();
        assert!(matches!(
            events.as_slice(),
            [
                AgentEvent::BackgroundTaskEnded { task, .. },
                AgentEvent::ToolFinished { is_error: true, .. },
            ] if task.status == TaskStatus::Stopped
        ));
        assert!(!background.keeps_stdin_open());
    }

    /// Replays captured Codex lines the way the reader handles them: a call that starts a command
    /// is a task that reports no end, and its turn completing lists the tasks still open.
    #[test]
    fn a_codex_command_open_at_turn_end_is_listed_and_holds_no_stdin() {
        let spec = TurnSpec {
            root: "/abs/work",
            session_id: None,
            model: None,
            vision_model: None,
            text: "go",
            mode: AccessMode::FullAccess,
            plan: false,
            effort: None,
            attachments: &[],
            images: &[],
            instructions: None,
            skills_dir: None,
            guarded: &[],
            agents: &[],
        };
        let mut background = Background::default();
        let mut events = Vec::new();
        for line in [
            codex_fixtures::QUICK_STARTED,
            codex_fixtures::QUICK_COMPLETED,
            codex_fixtures::COMMAND_STARTED,
            codex_fixtures::TURN_COMPLETED,
        ] {
            let value = serde_json::from_str(line).unwrap();
            for item in codex::parse(&value, &spec, "m1") {
                match item {
                    Parsed::Event(AgentEvent::ToolStarted { tool, .. }) => {
                        if let Some(task) = codex::background_task(&tool) {
                            background.start_unreported(task);
                        }
                    }
                    Parsed::Event(AgentEvent::ToolFinished {
                        tool_id, is_error, ..
                    }) => events.extend(background.finish_unreported(&tool_id, is_error)),
                    Parsed::Finished => events.extend(background.list_unreported("m1")),
                    _ => {}
                }
            }
        }
        // Only the command still running at turn end is listed; the quick one finished.
        assert!(matches!(
            events.as_slice(),
            [AgentEvent::BackgroundTaskStarted { task, .. }]
                if task.tool_id == "exec-d7e8f3a5-1046-4c24-9b3b-f176ac51ae9c" && task.kind == "command"
        ));
        // Codex never reports it ending, so nothing waits on it: stdin closes with the turn.
        assert!(!background.keeps_stdin_open());
    }
}
