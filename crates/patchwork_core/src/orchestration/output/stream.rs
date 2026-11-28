//! Streaming agent output to the frontend, and saving events onto the stored chat.

// Rust guideline compliant 2026-11-23

use crate::chats::{MessageExt};
use std::sync::Arc;
use crate::Error;
use crate::Core;
use crate::events::ChatEvent;
use crate::events::ChatEventEnvelope;
use crate::vault::requests::SecretStatus;
use crate::chats::ApprovalStatus;
use crate::chats::Message;
use crate::chats::MessageStatus;
use crate::system::now_ms;
use crate::chats::Part;
use crate::chats::StepStatus;
use crate::chats::Chat;
use crate::orchestration::turn::start::Turn;

impl Turn {
    /// Streams a text delta to the frontend and adds it to the chat in memory, so a reload
    /// mid-reply shows all of it; `unsaved` marks it for one disk write later.
    pub(in crate::orchestration) fn stream_text(&self, event: ChatEvent, unsaved: &mut Unsaved) {
        self.save_thinking(unsaved);
        if let ChatEvent::TextDelta { .. } = &event {
            self.core
                .store
                .update_unsaved(&self.chat_id, |t| apply(t, &event));
            unsaved.text = true;
        }
        self.emit(event);
    }

    /// Streams reasoning like `stream_text`, keeping the in-memory store in sync so reloads
    /// mid-turn do not drop it; it is persisted to disk when it ends.
    pub(in crate::orchestration) fn stream_thinking(
        &self,
        event: ChatEvent,
        unsaved: &mut Unsaved,
    ) {
        self.save_only_text(unsaved);
        if let ChatEvent::Thinking { .. } = &event {
            self.core
                .store
                .update_unsaved(&self.chat_id, |t| apply(t, &event));
            unsaved.thinking = true;
        }
        self.emit(event);
    }

    pub(in crate::orchestration) fn emit(&self, event: ChatEvent) {
        self.core.emit(ChatEventEnvelope {
            chat_id: self.chat_id.clone(),
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
        let saved = self
            .core
            .store
            .update_message(&self.chat_id, Some(&self.message_id), |_| {});
        if let Err(e) = saved {
            log::warn!(error:% = e; "failed to save agent text: {{error}}");
        }
    }

    /// Saves the reasoning block that streamed and marks it ended.
    pub(in crate::orchestration) fn save_thinking(&self, unsaved: &mut Unsaved) {
        if !std::mem::take(&mut unsaved.thinking) {
            return;
        }
        let saved = self
            .core
            .store
            .update_message(&self.chat_id, Some(&self.message_id), |chat| {
                if let Some(msg) = chat
                    .messages
                    .iter_mut()
                    .find(|m| m.id == self.message_id)
                    .map(Arc::make_mut)
                {
                    end_thinking(msg);
                }
            });
        if let Err(e) = saved {
            log::warn!(error:% = e; "failed to save agent reasoning: {{error}}");
        }
    }
}

/// Saves `event` onto the stored message, then streams it to the frontend.
pub(crate) fn publish(core: &Core, chat_id: &str, event: ChatEvent) {
    let event = settle_late(core, chat_id, event);
    if let Err(e) = save_event(core, chat_id, &event) {
        log::warn!(error:% = e; "failed to save agent event: {{error}}");
    }
    core.emit(ChatEventEnvelope {
        chat_id: chat_id.into(),
        event,
    });
}

/// A tool that starts after its reply has ended is settled as the end settles the others, rather
/// than left running. A late question stays open: the agent is waiting on it, and it can still be
/// answered.
fn settle_late(core: &Core, chat_id: &str, mut event: ChatEvent) -> ChatEvent {
    let ChatEvent::ToolStarted { message_id, tool } = &mut event else {
        return event;
    };
    // A swarm worker's call is the swarm's to show: it reads running again each time its worker
    // goes back to work, and the swarm ends it when the agent's process does. A reply that ends
    // while its workers are at work then still shows which of them are.
    if tool
        .id
        .starts_with(crate::provider::antigravity::SWARM_TASK_PREFIX)
    {
        return event;
    }
    let ended = core.store.with_chats(|chats| {
        chats
            .iter()
            .find(|t| t.id == chat_id)
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

/// Streamed output not yet stored: whether text or reasoning is in memory only.
#[derive(Default)]
pub(in crate::orchestration) struct Unsaved {
    pub(in crate::orchestration) text: bool,
    pub(in crate::orchestration) thinking: bool,
}

/// Applies `event` to the stored chat. Removing a message moves the ones after it, so
/// that goes through `update_chat`; every other event changes at most one message in place.
pub(in crate::orchestration) fn save_event(
    core: &Core,
    chat_id: &str,
    event: &ChatEvent,
) -> Result<(), Error> {
    if let ChatEvent::MessageRemoved { .. } = event {
        return core.store.update_chat(chat_id, |t| apply(t, event));
    }
    let message_id = if let ChatEvent::MessageStarted { message } = event {
        Some(message.id.as_str())
    } else {
        event.message_id()
    };
    core.store
        .update_message(chat_id, message_id, |t| apply(t, event))
}

/// Ends the reasoning block still streaming, if any, as the next step starts.
pub(in crate::orchestration) fn end_thinking(msg: &mut Message) {
    if let Some(Part::Thinking { ended_at, .. }) = msg.parts.last_mut() {
        ended_at.get_or_insert_with(now_ms);
    }
}

pub(in crate::orchestration) fn apply(chat: &mut Chat, event: &ChatEvent) {
    match event {
        ChatEvent::MessageStarted { message } => {
            if !chat.messages.iter().any(|m| m.id == message.id) {
                chat.messages.push(Arc::new(message.clone()));
            }
            return;
        }
        ChatEvent::GoalUpdated { goal } => {
            chat.goal.clone_from(goal);
            return;
        }
        ChatEvent::SlashCommandsUpdated { commands } => {
            chat.slash_commands.clone_from(commands);
            return;
        }
        ChatEvent::RoleChanged { role } => {
            chat.role.clone_from(role);
            return;
        }
        ChatEvent::ModelChanged { model } => {
            chat.model.clone_from(model);
            return;
        }
        ChatEvent::TitleUpdated { title } => {
            chat.title.clone_from(title);
            return;
        }
        ChatEvent::MessageRemoved { message_id } => {
            chat.messages.retain(|m| m.id != *message_id);
            return;
        }
        ChatEvent::MessageEdited { message } => {
            if let Some(stored) = chat.messages.iter_mut().find(|m| m.id == message.id) {
                *stored = Arc::new(message.clone());
            }
            return;
        }
        _ => {}
    }
    let Some(message_id) = event.message_id() else {
        return;
    };
    let Some(msg) = chat
        .messages
        .iter_mut()
        .find(|m| m.id == *message_id)
        .map(Arc::make_mut)
    else {
        return;
    };
    match event {
        ChatEvent::TextDelta { text, .. } => {
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
        ChatEvent::Thinking {
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
        // A call shown while its input was still streaming takes its full form in place.
        ChatEvent::ToolStarted { tool, .. } => {
            if let Some(shown) = msg.tools.iter_mut().find(|t| t.id == tool.id) {
                let started_at = shown.started_at;
                // The chat it opened is its own, whichever update says how it reads now: a
                // swarm worker's call is shown again each time its worker goes back to work.
                let child_chat_id = shown.child_chat_id.clone();
                *shown = tool.clone();
                shown.started_at = started_at.or(tool.started_at);
                shown.child_chat_id = shown.child_chat_id.clone().or(child_chat_id);
                return;
            }
            end_thinking(msg);
            msg.parts.push(Part::Tool {
                id: tool.id.clone(),
            });
            msg.tools.push(tool.clone());
        }
        ChatEvent::ToolFinished {
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
        ChatEvent::PlanUpdated { steps, .. } => msg.plan.clone_from(steps),
        ChatEvent::Usage { usage, .. } => msg.usage = Some(*usage),
        ChatEvent::MessageQueued { queued, .. } => {
            msg.queued = *queued;
            // A message waiting in the queue was not taken by the agent, so it is not a steer.
            if *queued {
                msg.steer = false;
            }
        }
        ChatEvent::MessageSteered { .. } => {
            msg.queued = false;
            msg.steer = true;
        }
        ChatEvent::MessageCutOff { .. } => msg.cut_off = true,
        ChatEvent::Refused { refusal, .. } => msg.refusal = Some(refusal.clone()),
        ChatEvent::Cursor { cursor, .. } => msg.cursor = Some(cursor.clone()),
        ChatEvent::PlanProposed { text, .. } => {
            end_thinking(msg);
            msg.parts.push(Part::Plan { text: text.clone() });
        }
        // One line per compaction, however often the agent reports it is still at it.
        ChatEvent::Compacting { .. } => {
            end_thinking(msg);
            if !matches!(msg.parts.last(), Some(Part::Compaction { .. })) {
                msg.parts.push(Part::Compaction {
                    summary: None,
                    done: false,
                });
            }
        }
        ChatEvent::Compacted { summary, .. } => {
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
        ChatEvent::BackgroundTaskStarted { task, .. }
        | ChatEvent::BackgroundTaskUpdated { task, .. }
        | ChatEvent::BackgroundTaskEnded { task, .. } => {
            match msg
                .background
                .iter_mut()
                .find(|t| t.tool_id == task.tool_id)
            {
                Some(listed) => *listed = task.clone(),
                None => msg.background.push(task.clone()),
            }
        }
        ChatEvent::FileTouched { path, .. } => {
            if !msg.files.contains(path) {
                msg.files.push(path.clone());
            }
        }
        ChatEvent::TurnRecorded { turn, .. } => msg.turn = Some(turn.clone()),
        ChatEvent::ChecksRecorded { checks, .. } => msg.checks = Some(checks.clone()),
        ChatEvent::ApprovalRequested { approval, .. } => {
            end_thinking(msg);
            msg.parts.push(Part::Approval {
                id: approval.id.clone(),
            });
            msg.approvals.push(approval.clone());
        }
        ChatEvent::ApprovalResolved {
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
        ChatEvent::SecretResolved {
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
        ChatEvent::TurnCompleted { status, error, .. } => {
            end_thinking(msg);
            msg.status = *status;
            msg.error.clone_from(error);
            msg.finished_at = Some(now_ms());
            msg.hide_secrets();
            // Unanswered requests lapse with the turn, but for a question the next message
            // answers.
            msg.approvals
                .iter_mut()
                .filter(|a| a.status == ApprovalStatus::Pending && !a.by_message)
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
        ChatEvent::MessageStarted { .. }
        | ChatEvent::GoalUpdated { .. }
        | ChatEvent::Retrying { .. }
        | ChatEvent::Quiet { .. }
        | ChatEvent::SlashCommandsUpdated { .. }
        | ChatEvent::RoleChanged { .. }
        | ChatEvent::ModelChanged { .. }
        | ChatEvent::ChatCreated { .. }
        | ChatEvent::ChatDeleted
        | ChatEvent::TitleUpdated { .. }
        | ChatEvent::ChatAppsChanged
        | ChatEvent::OptionsChanged
        | ChatEvent::MessageRemoved { .. }
        | ChatEvent::MessageEdited { .. } => {}
    }
}
