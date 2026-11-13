//! Streaming agent output to the frontend, and saving events onto the stored thread.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use crate::Error;
use crate::Server;
use crate::events::AgentEvent;
use crate::events::AgentEventEnvelope;
use crate::secrets::SecretStatus;
use crate::threads::ApprovalStatus;
use crate::threads::Message;
use crate::threads::MessageStatus;
use crate::system::now_ms;
use crate::threads::Part;
use crate::threads::StepStatus;
use crate::threads::Thread;
use crate::orchestration::turn::start::Turn;

impl Turn {
    /// Streams a text delta to the frontend and adds it to the thread in memory, so a reload
    /// mid-reply shows all of it; `unsaved` marks it for one disk write later.
    pub(in crate::orchestration) fn stream_text(&self, event: AgentEvent, unsaved: &mut Unsaved) {
        self.save_thinking(unsaved);
        if let AgentEvent::TextDelta { .. } = &event {
            self.server
                .store
                .update_unsaved(&self.thread_id, |t| apply(t, &event));
            unsaved.text = true;
        }
        self.emit(event);
    }

    /// Streams reasoning like `stream_text`; it is stored as one block when it ends.
    pub(in crate::orchestration) fn stream_thinking(
        &self,
        event: AgentEvent,
        unsaved: &mut Unsaved,
    ) {
        self.save_only_text(unsaved);
        if let AgentEvent::Thinking { text, .. } = &event {
            if unsaved.thinking.is_empty() {
                unsaved.thinking_since = now_ms();
            }
            unsaved.thinking.push_str(text);
        }
        self.emit(event);
    }

    pub(in crate::orchestration) fn emit(&self, event: AgentEvent) {
        self.server.emit(AgentEventEnvelope {
            thread_id: self.thread_id.clone(),
            event,
        });
    }

    /// Saves text and reasoning that were streamed but not yet stored.
    pub(in crate::orchestration) fn save_text(&self, unsaved: &mut Unsaved) {
        self.save_thinking(unsaved);
        self.save_only_text(unsaved);
    }

    pub(in crate::orchestration) fn save_only_text(&self, unsaved: &mut Unsaved) {
        if !std::mem::take(&mut unsaved.text) {
            return;
        }
        let saved =
            self.server
                .store
                .update_message(&self.thread_id, Some(&self.message_id), |_| {});
        if let Err(e) = saved {
            log::warn!(error:% = e; "failed to save agent text: {{error}}");
        }
    }

    /// Saves the reasoning block that streamed since `thinking_since`; it ends now.
    pub(in crate::orchestration) fn save_thinking(&self, unsaved: &mut Unsaved) {
        if unsaved.thinking.is_empty() {
            return;
        }
        let event = AgentEvent::Thinking {
            message_id: self.message_id.clone(),
            text: std::mem::take(&mut unsaved.thinking),
            started_at: Some(unsaved.thinking_since),
            ended_at: Some(now_ms()),
        };
        if let Err(e) = save_event(&self.server, &self.thread_id, &event) {
            log::warn!(error:% = e; "failed to save agent reasoning: {{error}}");
        }
    }
}

/// Saves `event` onto the stored message, then streams it to the frontend.
pub(crate) fn publish(server: &Server, thread_id: &str, event: AgentEvent) {
    let event = settle_late(server, thread_id, event);
    if let Err(e) = save_event(server, thread_id, &event) {
        log::warn!(error:% = e; "failed to save agent event: {{error}}");
    }
    server.emit(AgentEventEnvelope {
        thread_id: thread_id.into(),
        event,
    });
}

/// A tool that starts after its reply has ended is settled as the end settles the others, rather
/// than left running. A late question stays open: the agent is waiting on it, and it can still be
/// answered.
fn settle_late(server: &Server, thread_id: &str, mut event: AgentEvent) -> AgentEvent {
    let AgentEvent::ToolStarted { message_id, tool } = &mut event else {
        return event;
    };
    let ended = server.store.with_threads(|threads| {
        threads
            .iter()
            .find(|t| t.id == thread_id)
            .and_then(|t| t.messages.iter().find(|m| m.id == *message_id))
            .filter(|m| m.finished_at.is_some())
            .map(|m| m.status)
    });
    if let Some(status) = ended {
        tool.status = if status == MessageStatus::Complete {
            StepStatus::Done
        } else {
            StepStatus::Error
        };
        tool.ended_at = Some(now_ms());
    }
    event
}

/// Streamed output not yet stored: whether text is in memory only, or a block of reasoning and
/// when it started.
#[derive(Default)]
pub(in crate::orchestration) struct Unsaved {
    pub(in crate::orchestration) text: bool,
    pub(in crate::orchestration) thinking: String,
    pub(in crate::orchestration) thinking_since: u64,
}

/// Applies `event` to the stored thread. Removing a message moves the ones after it, so
/// that goes through `update_thread`; every other event changes at most one message in place.
pub(in crate::orchestration) fn save_event(
    server: &Server,
    thread_id: &str,
    event: &AgentEvent,
) -> Result<(), Error> {
    if let AgentEvent::MessageRemoved { .. } = event {
        return server.store.update_thread(thread_id, |t| apply(t, event));
    }
    let message_id = if let AgentEvent::MessageStarted { message } = event {
        Some(message.id.as_str())
    } else {
        event_message(event)
    };
    server
        .store
        .update_message(thread_id, message_id, |t| apply(t, event))
}

/// The existing message `event` changes, if it changes one.
pub(in crate::orchestration) fn event_message(event: &AgentEvent) -> Option<&str> {
    match event {
        AgentEvent::MessageStarted { .. }
        | AgentEvent::GoalUpdated { .. }
        | AgentEvent::SlashCommandsUpdated { .. }
        | AgentEvent::RoleChanged { .. }
        | AgentEvent::ModelChanged { .. }
        | AgentEvent::ThreadCreated { .. }
        | AgentEvent::ThreadDeleted
        | AgentEvent::TitleUpdated { .. }
        | AgentEvent::MessageRemoved { .. } => None,
        AgentEvent::MessageEdited { message } => Some(&message.id),
        AgentEvent::TextDelta { message_id, .. }
        | AgentEvent::Thinking { message_id, .. }
        | AgentEvent::ToolStarted { message_id, .. }
        | AgentEvent::ToolFinished { message_id, .. }
        | AgentEvent::PlanUpdated { message_id, .. }
        | AgentEvent::FileTouched { message_id, .. }
        | AgentEvent::TurnRecorded { message_id, .. }
        | AgentEvent::ChecksRecorded { message_id, .. }
        | AgentEvent::ReviewRecorded { message_id, .. }
        | AgentEvent::ApprovalRequested { message_id, .. }
        | AgentEvent::ApprovalResolved { message_id, .. }
        | AgentEvent::SecretResolved { message_id, .. }
        | AgentEvent::Usage { message_id, .. }
        | AgentEvent::MessageQueued { message_id, .. }
        | AgentEvent::MessageSteered { message_id }
        | AgentEvent::MessageCutOff { message_id }
        | AgentEvent::GoalChecking { message_id }
        | AgentEvent::Compacting { message_id }
        | AgentEvent::Compacted { message_id, .. }
        | AgentEvent::Retrying { message_id, .. }
        | AgentEvent::Quiet { message_id, .. }
        | AgentEvent::Refused { message_id, .. }
        | AgentEvent::Cursor { message_id, .. }
        | AgentEvent::PlanProposed { message_id, .. }
        | AgentEvent::BackgroundTaskStarted { message_id, .. }
        | AgentEvent::BackgroundTaskUpdated { message_id, .. }
        | AgentEvent::BackgroundTaskEnded { message_id, .. }
        | AgentEvent::TurnCompleted { message_id, .. } => Some(message_id),
    }
}

/// Ends the reasoning block still streaming, if any, as the next step starts.
pub(in crate::orchestration) fn end_thinking(msg: &mut Message) {
    if let Some(Part::Thinking { ended_at, .. }) = msg.parts.last_mut() {
        ended_at.get_or_insert_with(now_ms);
    }
}

pub(in crate::orchestration) fn apply(thread: &mut Thread, event: &AgentEvent) {
    match event {
        AgentEvent::MessageStarted { message } => {
            if !thread.messages.iter().any(|m| m.id == message.id) {
                thread.messages.push(Arc::new(message.clone()));
            }
            return;
        }
        AgentEvent::GoalUpdated { goal } => {
            thread.goal.clone_from(goal);
            return;
        }
        AgentEvent::SlashCommandsUpdated { commands } => {
            thread.slash_commands.clone_from(commands);
            return;
        }
        AgentEvent::RoleChanged { role } => {
            thread.role.clone_from(role);
            return;
        }
        AgentEvent::ModelChanged { model } => {
            thread.model.clone_from(model);
            return;
        }
        AgentEvent::TitleUpdated { title } => {
            thread.title.clone_from(title);
            return;
        }
        AgentEvent::MessageRemoved { message_id } => {
            thread.messages.retain(|m| m.id != *message_id);
            return;
        }
        AgentEvent::MessageEdited { message } => {
            if let Some(stored) = thread.messages.iter_mut().find(|m| m.id == message.id) {
                *stored = Arc::new(message.clone());
            }
            return;
        }
        _ => {}
    }
    let Some(message_id) = event_message(event) else {
        return;
    };
    let Some(msg) = thread
        .messages
        .iter_mut()
        .find(|m| m.id == *message_id)
        .map(Arc::make_mut)
    else {
        return;
    };
    match event {
        AgentEvent::TextDelta { text, .. } => {
            msg.text.push_str(text);
            end_thinking(msg);
            match msg.parts.last_mut() {
                Some(Part::Text { text: last }) => last.push_str(text),
                None
                | Some(
                    Part::Thinking { .. }
                    | Part::Tool { .. }
                    | Part::Plan { .. }
                    | Part::Approval { .. }
                    | Part::Compaction { .. },
                ) => msg.parts.push(Part::Text { text: text.clone() }),
            }
        }
        AgentEvent::Thinking {
            text,
            started_at,
            ended_at,
            ..
        } => match msg.parts.last_mut() {
            Some(Part::Thinking {
                text: last,
                ended_at: None,
                ..
            }) => last.push_str(text),
            _ => msg.parts.push(Part::Thinking {
                text: text.clone(),
                started_at: started_at.unwrap_or_else(now_ms),
                ended_at: *ended_at,
            }),
        },
        AgentEvent::ToolStarted { tool, .. } => {
            end_thinking(msg);
            msg.parts.push(Part::Tool {
                id: tool.id.clone(),
            });
            msg.tools.push(tool.clone());
        }
        AgentEvent::ToolFinished {
            tool_id,
            is_error,
            output,
            ..
        } => {
            if let Some(tool) = msg.tools.iter_mut().find(|t| t.id == *tool_id) {
                tool.output.clone_from(output);
                tool.ended_at = Some(now_ms());
                tool.status = if *is_error {
                    StepStatus::Error
                } else {
                    StepStatus::Done
                };
            }
        }
        AgentEvent::PlanUpdated { steps, .. } => msg.plan.clone_from(steps),
        AgentEvent::Usage { usage, .. } => msg.usage = Some(*usage),
        AgentEvent::MessageQueued { queued, .. } => {
            msg.queued = *queued;
            // A message waiting in the queue was not taken by the agent, so it is not a steer.
            if *queued {
                msg.steer = false;
            }
        }
        AgentEvent::MessageSteered { .. } => {
            msg.queued = false;
            msg.steer = true;
        }
        AgentEvent::MessageCutOff { .. } => msg.cut_off = true,
        AgentEvent::Refused { refusal, .. } => msg.refusal = Some(refusal.clone()),
        AgentEvent::Cursor { cursor, .. } => msg.cursor = Some(cursor.clone()),
        AgentEvent::PlanProposed { text, .. } => {
            end_thinking(msg);
            msg.parts.push(Part::Plan { text: text.clone() });
        }
        // One line per compaction, however often the agent reports it is still at it.
        AgentEvent::Compacting { .. } => {
            end_thinking(msg);
            if !matches!(msg.parts.last(), Some(Part::Compaction { .. })) {
                msg.parts.push(Part::Compaction {
                    summary: None,
                    done: false,
                });
            }
        }
        AgentEvent::Compacted { summary, .. } => {
            if let Some(Part::Compaction {
                summary: last,
                done,
            }) = msg
                .parts
                .iter_mut()
                .rev()
                .find(|p| matches!(p, Part::Compaction { .. }))
            {
                if summary.is_some() {
                    last.clone_from(summary);
                }
                *done = true;
            }
        }
        // A task stays on its message once it ends, with its final status; the strip shows only
        // the ones still live.
        AgentEvent::BackgroundTaskStarted { task, .. }
        | AgentEvent::BackgroundTaskUpdated { task, .. }
        | AgentEvent::BackgroundTaskEnded { task, .. } => {
            match msg
                .background
                .iter_mut()
                .find(|t| t.tool_id == task.tool_id)
            {
                Some(listed) => *listed = task.clone(),
                None => msg.background.push(task.clone()),
            }
        }
        AgentEvent::FileTouched { path, .. } => {
            if !msg.files.contains(path) {
                msg.files.push(path.clone());
            }
        }
        AgentEvent::TurnRecorded { turn, .. } => msg.turn = Some(turn.clone()),
        AgentEvent::ChecksRecorded { checks, .. } => msg.checks = Some(checks.clone()),
        AgentEvent::ReviewRecorded { review, .. } => msg.review = Some(review.clone()),
        AgentEvent::ApprovalRequested { approval, .. } => {
            end_thinking(msg);
            msg.parts.push(Part::Approval {
                id: approval.id.clone(),
            });
            msg.approvals.push(approval.clone());
        }
        AgentEvent::ApprovalResolved {
            approval_id,
            allowed,
            answers,
            rule,
            auto,
            ..
        } => {
            if let Some(approval) = msg.approvals.iter_mut().find(|a| a.id == *approval_id) {
                approval.answers.clone_from(answers);
                approval.rule.clone_from(rule);
                approval.auto = *auto;
                approval.status = if *allowed {
                    ApprovalStatus::Allowed
                } else {
                    ApprovalStatus::Denied
                };
            }
        }
        AgentEvent::SecretResolved {
            approval_id,
            status,
            ..
        } => {
            if let Some(approval) = msg.approvals.iter_mut().find(|a| a.id == *approval_id) {
                approval.status = if *status == SecretStatus::Saved {
                    ApprovalStatus::Allowed
                } else {
                    ApprovalStatus::Denied
                };
                if let Some(secret) = &mut approval.secret {
                    secret.status = *status;
                }
            }
        }
        AgentEvent::TurnCompleted { status, error, .. } => {
            end_thinking(msg);
            msg.status = *status;
            msg.error.clone_from(error);
            msg.finished_at = Some(now_ms());
            // Unanswered requests lapse with the turn.
            msg.approvals
                .iter_mut()
                .filter(|a| a.status == ApprovalStatus::Pending)
                .for_each(|approval| {
                    approval.status = ApprovalStatus::Denied;
                    if let Some(secret) = &mut approval.secret {
                        secret.status = SecretStatus::Cancelled;
                    }
                });
            // Nothing can still be running once the turn has ended, except a call whose
            // background task outlives the reply, such as a subagent: it is shown finished when
            // its task ends.
            let tool_end = if *status == MessageStatus::Complete {
                StepStatus::Done
            } else {
                StepStatus::Error
            };
            let background = &msg.background;
            msg.tools
                .iter_mut()
                .filter(|t| t.status == StepStatus::Running)
                .filter(|t| {
                    !background
                        .iter()
                        .any(|task| task.tool_id == t.id && task.status.is_live())
                })
                .for_each(|tool| {
                    tool.status = tool_end;
                    tool.ended_at = Some(now_ms());
                });
            msg.plan
                .iter_mut()
                .filter(|s| s.status == StepStatus::Running)
                .for_each(|step| step.status = StepStatus::Pending);
        }
        AgentEvent::MessageStarted { .. }
        | AgentEvent::GoalUpdated { .. }
        | AgentEvent::GoalChecking { .. }
        | AgentEvent::Retrying { .. }
        | AgentEvent::Quiet { .. }
        | AgentEvent::SlashCommandsUpdated { .. }
        | AgentEvent::RoleChanged { .. }
        | AgentEvent::ModelChanged { .. }
        | AgentEvent::ThreadCreated { .. }
        | AgentEvent::ThreadDeleted
        | AgentEvent::TitleUpdated { .. }
        | AgentEvent::MessageRemoved { .. }
        | AgentEvent::MessageEdited { .. } => {}
    }
}
