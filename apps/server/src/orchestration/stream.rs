//! Streaming agent output to the frontend, and saving events onto the stored thread.
use crate::Res;
use crate::Server;
use crate::types::AgentEvent;
use crate::types::AgentEventEnvelope;
use crate::types::ApprovalStatus;
use crate::types::Message;
use crate::types::MessageStatus;
use crate::types::now_ms;
use crate::types::Part;
use crate::types::StepStatus;
use crate::types::Thread;
use super::turn::Turn;


impl Turn {
    /// Streams a text delta to the frontend without saving it; the reader
    /// collects it in `unsaved` so it is stored in one write later.
    pub(super) fn stream_text(&self, event: AgentEvent, unsaved: &mut Unsaved) {
        self.save_thinking(unsaved);
        if let AgentEvent::TextDelta { text, .. } = &event {
            unsaved.text.push_str(text);
        }
        self.emit(event);
    }

    /// Streams reasoning like `stream_text`; it is stored as one block when it ends.
    pub(super) fn stream_thinking(&self, event: AgentEvent, unsaved: &mut Unsaved) {
        self.save_only_text(unsaved);
        if let AgentEvent::Thinking { text, .. } = &event {
            if unsaved.thinking.is_empty() {
                unsaved.thinking_since = now_ms();
            }
            unsaved.thinking.push_str(text);
        }
        self.emit(event);
    }

    pub(super) fn emit(&self, event: AgentEvent) {
        self.server.emit(AgentEventEnvelope {
            thread_id: self.thread_id.clone(),
            event,
        });
    }

    /// Saves text and reasoning that were streamed but not yet stored.
    pub(super) fn save_text(&self, unsaved: &mut Unsaved) {
        self.save_thinking(unsaved);
        self.save_only_text(unsaved);
    }

    pub(super) fn save_only_text(&self, unsaved: &mut Unsaved) {
        if unsaved.text.is_empty() {
            return;
        }
        let event = AgentEvent::TextDelta {
            message_id: self.message_id.clone(),
            text: std::mem::take(&mut unsaved.text),
        };
        if let Err(e) = save_event(&self.server, &self.thread_id, &event) {
            log::warn!("failed to save agent text: {e}");
        }
    }

    /// Saves the reasoning block that streamed since `thinking_since`; it ends now.
    pub(super) fn save_thinking(&self, unsaved: &mut Unsaved) {
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
            log::warn!("failed to save agent reasoning: {e}");
        }
    }
}

/// Saves `event` onto the stored message, then streams it to the frontend.
pub(crate) fn publish(server: &Server, thread_id: &str, event: AgentEvent) {
    if let Err(e) = save_event(server, thread_id, &event) {
        log::warn!("failed to save agent event: {e}");
    }
    server.emit(AgentEventEnvelope {
        thread_id: thread_id.into(),
        event,
    });
}

/// Streamed output not yet stored: text, or a block of reasoning and when it started.
#[derive(Default)]
pub(super) struct Unsaved {
    pub(super) text: String,
    pub(super) thinking: String,
    pub(super) thinking_since: u64,
}

/// Applies one event to the stored message. Used for both persistence and replay.
/// Applies `event` to the stored thread. Removing a message moves the ones after it, so
/// that goes through `update_thread`; every other event changes at most one message in place.
pub(super) fn save_event(server: &Server, thread_id: &str, event: &AgentEvent) -> Res<()> {
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
pub(super) fn event_message(event: &AgentEvent) -> Option<&str> {
    match event {
        AgentEvent::MessageStarted { .. }
        | AgentEvent::GoalUpdated { .. }
        | AgentEvent::SlashCommandsUpdated { .. }
        | AgentEvent::ThreadCreated { .. }
        | AgentEvent::MessageRemoved { .. } => None,
        AgentEvent::MessageEdited { message } => Some(&message.id),
        AgentEvent::TextDelta { message_id, .. }
        | AgentEvent::Thinking { message_id, .. }
        | AgentEvent::ToolStarted { message_id, .. }
        | AgentEvent::ToolFinished { message_id, .. }
        | AgentEvent::PlanUpdated { message_id, .. }
        | AgentEvent::FileTouched { message_id, .. }
        | AgentEvent::ApprovalRequested { message_id, .. }
        | AgentEvent::ApprovalResolved { message_id, .. }
        | AgentEvent::Usage { message_id, .. }
        | AgentEvent::MessageQueued { message_id, .. }
        | AgentEvent::MessageSteered { message_id }
        | AgentEvent::MessageCutOff { message_id }
        | AgentEvent::PlanProposed { message_id, .. }
        | AgentEvent::BackgroundTaskStarted { message_id, .. }
        | AgentEvent::BackgroundTaskEnded { message_id, .. }
        | AgentEvent::TurnCompleted { message_id, .. } => Some(message_id),
    }
}

/// Ends the reasoning block still streaming, if any, as the next step starts.
pub(super) fn end_thinking(msg: &mut Message) {
    if let Some(Part::Thinking { ended_at, .. }) = msg.parts.last_mut() {
        ended_at.get_or_insert_with(now_ms);
    }
}

pub(super) fn apply(thread: &mut Thread, event: &AgentEvent) {
    match event {
        AgentEvent::MessageStarted { message } => {
            if !thread.messages.iter().any(|m| m.id == message.id) {
                thread.messages.push(message.clone());
            }
            return;
        }
        AgentEvent::GoalUpdated { goal } => {
            thread.goal = goal.clone();
            return;
        }
        AgentEvent::SlashCommandsUpdated { commands } => {
            thread.slash_commands = commands.clone();
            return;
        }
        AgentEvent::MessageRemoved { message_id } => {
            thread.messages.retain(|m| m.id != *message_id);
            return;
        }
        AgentEvent::MessageEdited { message } => {
            if let Some(stored) = thread.messages.iter_mut().find(|m| m.id == message.id) {
                *stored = message.clone();
            }
            return;
        }
        _ => {}
    }
    let Some(message_id) = event_message(event) else {
        return;
    };
    let Some(msg) = thread.messages.iter_mut().find(|m| m.id == *message_id) else {
        return;
    };
    match event {
        AgentEvent::TextDelta { text, .. } => {
            msg.text.push_str(text);
            end_thinking(msg);
            match msg.parts.last_mut() {
                Some(Part::Text { text: last }) => last.push_str(text),
                None
                | Some(Part::Thinking { .. } | Part::Tool { .. } | Part::Plan { .. }) => {
                    msg.parts.push(Part::Text { text: text.clone() })
                }
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
        AgentEvent::PlanUpdated { steps, .. } => msg.plan = steps.clone(),
        AgentEvent::Usage { usage, .. } => msg.usage = Some(*usage),
        AgentEvent::MessageQueued { queued, .. } => msg.queued = *queued,
        AgentEvent::MessageSteered { .. } => { msg.queued = false; msg.steer = true; }
        AgentEvent::MessageCutOff { .. } => msg.cut_off = true,
        AgentEvent::PlanProposed { text, .. } => {
            end_thinking(msg);
            msg.parts.push(Part::Plan { text: text.clone() });
        }
        AgentEvent::BackgroundTaskStarted { task, .. } => msg.background.push(task.clone()),
        AgentEvent::BackgroundTaskEnded { tool_id, .. } => {
            msg.background.retain(|t| t.tool_id != *tool_id);
        }
        AgentEvent::FileTouched { path, .. } => {
            if !msg.files.contains(path) {
                msg.files.push(path.clone());
            }
        }
        AgentEvent::ApprovalRequested { approval, .. } => msg.approvals.push(approval.clone()),
        AgentEvent::ApprovalResolved {
            approval_id,
            allowed,
            answers,
            ..
        } => {
            if let Some(approval) = msg.approvals.iter_mut().find(|a| a.id == *approval_id) {
                approval.answers = answers.clone();
                approval.status = if *allowed {
                    ApprovalStatus::Allowed
                } else {
                    ApprovalStatus::Denied
                };
            }
        }
        AgentEvent::TurnCompleted { status, error, .. } => {
            end_thinking(msg);
            msg.status = *status;
            msg.error = error.clone();
            msg.finished_at = Some(now_ms());
            // Unanswered requests lapse with the turn.
            msg.approvals
                .iter_mut()
                .filter(|a| a.status == ApprovalStatus::Pending)
                .for_each(|approval| approval.status = ApprovalStatus::Denied);
            // Nothing can still be running once the turn has ended.
            let tool_end = if *status == MessageStatus::Complete {
                StepStatus::Done
            } else {
                StepStatus::Error
            };
            msg.tools
                .iter_mut()
                .filter(|t| t.status == StepStatus::Running)
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
        | AgentEvent::SlashCommandsUpdated { .. }
        | AgentEvent::ThreadCreated { .. }
        | AgentEvent::MessageRemoved { .. }
        | AgentEvent::MessageEdited { .. } => {}
    }
}
