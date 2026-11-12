//! Each event a chat streams, as a line in its own log (see `logging::THREAD_LOG`). Info has what a
//! reply did: its start and end, approvals, retries, goals, compactions, checks and background
//! tasks; Warning, what went wrong in it; Debug adds each tool call, plan, file and token count;
//! and Trace, the text the person and the agent wrote. Streamed text is left to the agent's own
//! traffic, which the chat's log also takes.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use crate::logging::{Data, Source, THREAD_LOG};
use crate::settings::LogLevel;
use crate::threads::{MessageStatus, Role, StepStatus};

use crate::events::{AgentEvent, AgentEventEnvelope};

/// Logs `envelope` to its chat's log.
pub(crate) fn log_event(envelope: &AgentEventEnvelope) {
    if !THREAD_LOG.logs(LogLevel::Error) {
        return;
    }
    let Some((level, event, message, text)) = describe(&envelope.event) else {
        return;
    };
    let source = Source {
        thread: Some(envelope.thread_id.clone()),
        message: message.map(String::from),
        ..Source::default()
    };
    THREAD_LOG.write(level, event, &source, Data::Text(text));
}

/// The level, name, reply and text of an event's line, or None for one the log leaves out.
fn describe(event: &AgentEvent) -> Option<(LogLevel, &'static str, Option<&str>, String)> {
    use LogLevel::{Debug, Error, Info, Warning};
    let trace = THREAD_LOG.logs(LogLevel::Trace);
    // The person's and the agent's own words are logged only at Trace.
    let words = |text: &str| {
        if trace {
            text.to_string()
        } else {
            format!("<{} chars>", text.chars().count())
        }
    };
    Some(match event {
        AgentEvent::MessageStarted { message } => {
            let role = match message.role {
                Role::User => "from the person",
                Role::Assistant => "from the agent",
            };
            let text = format!(
                "{role}, {:?}, {} attachments: {}",
                message.status,
                message.attachments.len(),
                words(&message.text)
            );
            (Info, "message started", Some(&message.id), text)
        }
        AgentEvent::TextDelta { .. } | AgentEvent::Thinking { .. } => return None,
        AgentEvent::ToolStarted { message_id, tool } => {
            let mut text = format!("{} {}", tool.name, tool.id);
            if let Some(parent) = &tool.parent {
                let _ = write!(text, " inside {parent}");
            }
            if trace {
                let _ = write!(text, ": {}", tool.summary);
            }
            let level = if tool.status == StepStatus::Error {
                Warning
            } else {
                Debug
            };
            (level, "tool started", Some(message_id), text)
        }
        AgentEvent::ToolFinished {
            message_id,
            tool_id,
            is_error,
            output,
        } => {
            if *is_error {
                // An error's output is how the agent learned what went wrong, so it is kept.
                let output = output.as_deref().unwrap_or("no output");
                (
                    Warning,
                    "tool failed",
                    Some(message_id),
                    format!("{tool_id}: {output}"),
                )
            } else {
                (Debug, "tool finished", Some(message_id), tool_id.clone())
            }
        }
        AgentEvent::PlanUpdated { message_id, steps } => {
            let done = steps
                .iter()
                .filter(|s| s.status == StepStatus::Done)
                .count();
            let text = format!("{done} of {} steps done", steps.len());
            (Debug, "plan updated", Some(message_id), text)
        }
        AgentEvent::FileTouched { message_id, path } => {
            (Debug, "file touched", Some(message_id), path.clone())
        }
        AgentEvent::ApprovalRequested {
            message_id,
            approval,
        } => {
            let mut text = format!("{} ({:?}, {})", approval.title, approval.kind, approval.id);
            if trace {
                let _ = write!(text, ": {}", approval.detail);
            }
            (Info, "approval requested", Some(message_id), text)
        }
        AgentEvent::ApprovalResolved {
            message_id,
            approval_id,
            allowed,
            rule,
            auto,
            ..
        } => {
            let answer = if *allowed { "allowed" } else { "denied" };
            let by = match (auto, rule) {
                (true, Some(rule)) => format!(" by rule {rule:?}"),
                (true, None) => " automatically".into(),
                (false, Some(rule)) => format!(" by the person, saving rule {rule:?}"),
                (false, None) => " by the person".into(),
            };
            (
                Info,
                "approval resolved",
                Some(message_id),
                format!("{approval_id} {answer}{by}"),
            )
        }
        AgentEvent::SecretResolved {
            message_id,
            approval_id,
            status,
        } => (
            Info,
            "secret request resolved",
            Some(message_id),
            format!("{approval_id} {status:?}"),
        ),
        AgentEvent::GoalUpdated { goal } => {
            let text = match goal {
                Some(goal) => format!(
                    "{:?}, {} unmet checks: {}",
                    goal.status,
                    goal.checks.unwrap_or(0),
                    words(&goal.objective)
                ),
                None => "cleared".into(),
            };
            (Info, "goal", None, text)
        }
        AgentEvent::GoalChecking { message_id } => {
            (Info, "goal checking", Some(message_id), String::new())
        }
        AgentEvent::Compacting { message_id } => {
            (Info, "compacting", Some(message_id), String::new())
        }
        AgentEvent::Compacted {
            message_id,
            summary,
        } => {
            let text = summary.as_deref().map(words).unwrap_or_default();
            (Info, "compacted", Some(message_id), text)
        }
        AgentEvent::Retrying {
            message_id,
            reason,
            attempt,
            max_attempts,
        } => {
            let text = format!("attempt {attempt} of {max_attempts}: {reason}");
            (Warning, "retrying", Some(message_id), text)
        }
        AgentEvent::SlashCommandsUpdated { commands } => (
            Debug,
            "slash commands",
            None,
            format!("{} commands", commands.len()),
        ),
        AgentEvent::RoleChanged { role } => (
            Info,
            "custom agent",
            None,
            role.clone().unwrap_or_else(|| "none".into()),
        ),
        AgentEvent::ModelChanged { model } => (
            Info,
            "model",
            None,
            model.clone().unwrap_or_else(|| "default".into()),
        ),
        AgentEvent::Usage { message_id, usage } => {
            let text = format!(
                "input {}, cache read {}, cache write {}, output {}, context {}",
                usage.input, usage.cache_read, usage.cache_write, usage.output, usage.context,
            );
            (Debug, "usage", Some(message_id), text)
        }
        AgentEvent::PlanProposed { message_id, text } => {
            (Info, "plan proposed", Some(message_id), words(text))
        }
        AgentEvent::MessageCutOff { message_id } => (
            Info,
            "cut off by a steering message",
            Some(message_id),
            String::new(),
        ),
        AgentEvent::Cursor { message_id, cursor } => {
            (Debug, "cursor", Some(message_id), cursor.clone())
        }
        AgentEvent::Refused {
            message_id,
            refusal,
        } => {
            let category = refusal.category.as_deref().unwrap_or("no category");
            let text = format!("{category}: {}", refusal.explanation);
            (Warning, "refused", Some(message_id), text)
        }
        AgentEvent::MessageQueued { message_id, queued } => {
            let text = if *queued {
                "queued"
            } else {
                "sent from the queue"
            };
            (Info, "message queued", Some(message_id), text.into())
        }
        AgentEvent::MessageSteered { message_id } => {
            (Info, "message steered", Some(message_id), String::new())
        }
        AgentEvent::MessageRemoved { message_id } => {
            (Info, "message removed", Some(message_id), String::new())
        }
        AgentEvent::MessageEdited { message } => (
            Info,
            "message edited",
            Some(&message.id),
            words(&message.text),
        ),
        AgentEvent::BackgroundTaskStarted { message_id, task }
        | AgentEvent::BackgroundTaskEnded { message_id, task } => {
            let event = match event {
                AgentEvent::BackgroundTaskStarted { .. } => "background task started",
                _ => "background task ended",
            };
            let text = format!("{} {:?} ({})", task.kind, task.status, task.tool_id);
            (Info, event, Some(message_id), text)
        }
        AgentEvent::BackgroundTaskUpdated { message_id, task } => {
            let text = format!("{} {:?} ({})", task.kind, task.status, task.tool_id);
            (Debug, "background task", Some(message_id), text)
        }
        AgentEvent::TurnRecorded { message_id, .. } => {
            (Debug, "changes recorded", Some(message_id), String::new())
        }
        AgentEvent::ChecksRecorded { message_id, checks } => {
            let text = format!(
                "{:?}, {} in {} ms",
                checks.status,
                checks.command.as_deref().unwrap_or("nothing to run"),
                checks.duration_ms
            );
            (Info, "checks", Some(message_id), text)
        }
        AgentEvent::TurnCompleted {
            message_id,
            status,
            error,
        } => {
            let level = match status {
                MessageStatus::Error => Error,
                MessageStatus::Cancelled | MessageStatus::Interrupted => Warning,
                _ => Info,
            };
            let mut text = format!("{status:?}");
            if let Some(error) = error {
                let _ = write!(text, ": {error}");
            }
            (level, "reply ended", Some(message_id), text)
        }
        AgentEvent::ThreadCreated { thread } => {
            let text = format!(
                "{:?}, model {}, {:?} access",
                thread.agent,
                thread.model.as_deref().unwrap_or("default"),
                thread.access_mode
            );
            (Info, "chat created", None, text)
        }
        AgentEvent::TitleUpdated { title } => (Debug, "title", None, title.clone()),
        AgentEvent::ThreadDeleted => (Info, "chat deleted", None, String::new()),
    })
}
